use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Resolve persisted titles without rereading growing Claude transcripts on
/// every poll. Only transcripts belonging to live Claude sessions are opened.
#[derive(Default)]
pub struct SessionNameScanner {
    transcripts: HashMap<PathBuf, TranscriptTitles>,
}

#[derive(Default)]
struct TranscriptTitles {
    offset: u64,
    inode: u64,
    generated: Option<String>,
    custom: Option<String>,
}

impl SessionNameScanner {
    pub fn scan(&mut self) -> HashMap<String, String> {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return HashMap::new();
        };
        let codex_home = std::env::var_os("CODEX_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let kimi_home = std::env::var_os("KIMI_CODE_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".kimi-code"));
        self.scan_paths(&home.join(".claude"), &codex_home, &kimi_home)
    }

    fn scan_paths(
        &mut self,
        claude_home: &Path,
        codex_home: &Path,
        kimi_home: &Path,
    ) -> HashMap<String, String> {
        let mut names = HashMap::new();
        let mut live_sessions = HashSet::new();
        let mut manual_names = HashMap::new();
        for path in entries(&claude_home.join("sessions")) {
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(value) = read_json(&path) else {
                continue;
            };
            let Some(id) = text(&value, "sessionId") else {
                continue;
            };
            live_sessions.insert(id.clone());
            if let Some(name) = text(&value, "name") {
                if value.get("nameSource").and_then(Value::as_str) != Some("derived") {
                    manual_names.insert(id.clone(), name.clone());
                }
                names.insert(id, name);
            }
        }

        let mut seen = HashSet::new();
        for project in entries(&claude_home.join("projects")) {
            for path in entries(&project) {
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !live_sessions.contains(id) {
                    continue;
                }
                seen.insert(path.clone());
                let titles = self.transcripts.entry(path.clone()).or_default();
                titles.refresh(&path, id);
                // Explicit renames win over generated titles; live-session
                // names remain the fallback before a title is generated.
                if let Some(title) = titles.custom.as_ref().or(titles.generated.as_ref()) {
                    names.insert(id.to_owned(), title.clone());
                }
            }
        }
        self.transcripts.retain(|path, _| seen.contains(path));
        names.extend(manual_names);

        // The index is append-only: the last valid name for an ID wins.
        if let Ok(file) = File::open(codex_home.join("session_index.jsonl")) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let (Some(id), Some(title)) = (text(&value, "id"), text(&value, "thread_name")) {
                    names.insert(id, title);
                }
            }
        }
        // Kimi's index identifies the state file; never infer a session by cwd.
        if let Ok(file) = File::open(kimi_home.join("session_index.jsonl")) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(id) = text(&value, "sessionId") else {
                    continue;
                };
                let Some(dir) = value.get("sessionDir").and_then(Value::as_str) else {
                    continue;
                };
                let Some(state) = read_json(&Path::new(dir).join("state.json")) else {
                    continue;
                };
                if state.get("id").and_then(Value::as_str) == Some(id.as_str())
                    && let Some(title) = text(&state, "title")
                {
                    names.insert(id, title);
                }
            }
        }
        names
    }
}

impl TranscriptTitles {
    fn refresh(&mut self, path: &Path, session_id: &str) {
        let Ok(mut file) = File::open(path) else {
            return;
        };
        let Ok(metadata) = file.metadata() else {
            return;
        };
        if metadata.ino() != self.inode || metadata.len() < self.offset {
            *self = Self {
                inode: metadata.ino(),
                ..Self::default()
            };
        }
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            let Ok(bytes) = reader.read_line(&mut line) else {
                break;
            };
            // Retry an incomplete last record on the next poll.
            if bytes == 0 || !line.ends_with('\n') {
                break;
            }
            self.offset += bytes as u64;
            // Avoid deserializing large conversation messages.
            if !line.contains("\"ai-title\"") && !line.contains("\"custom-title\"") {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("sessionId").and_then(Value::as_str) != Some(session_id) {
                continue;
            }
            match value.get("type").and_then(Value::as_str) {
                Some("ai-title") => {
                    if let Some(title) = text(&value, "aiTitle") {
                        self.generated = Some(title);
                    }
                }
                Some("custom-title") => {
                    if let Some(title) = text(&value, "customTitle") {
                        self.custom = Some(title);
                    }
                }
                _ => {}
            }
        }
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_reader(File::open(path).ok()?).ok()
}

fn text(value: &Value, key: &str) -> Option<String> {
    clean_title(value.get(key)?.as_str()?)
}

pub(crate) fn clean_title(text: &str) -> Option<String> {
    // Titles may contain a whole prompt. Keep them on one terminal row and
    // remove control characters before the renderer truncates to its width.
    let cleaned = text
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Startup read; the polling worker keeps its own scanner for incremental reads.
pub fn scan_session_names() -> HashMap<String, String> {
    SessionNameScanner::default().scan()
}

#[cfg(test)]
mod tests;
