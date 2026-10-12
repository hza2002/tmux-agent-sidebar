use super::*;
use std::io::Write;

struct Fixture {
    _dir: tempfile::TempDir,
    claude: PathBuf,
    codex: PathBuf,
    kimi: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        let codex = dir.path().join("codex");
        let kimi = dir.path().join("kimi");
        for path in [
            claude.join("sessions"),
            claude.join("projects/repo"),
            codex.clone(),
            kimi.clone(),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        Self {
            _dir: dir,
            claude,
            codex,
            kimi,
        }
    }

    fn write(&self, path: &Path, value: &str) {
        std::fs::write(path, value).unwrap();
    }

    fn scan(&self, scanner: &mut SessionNameScanner) -> HashMap<String, String> {
        scanner.scan_paths(&self.claude, &self.codex, &self.kimi)
    }
}

#[test]
fn native_titles_cover_all_file_sources_and_manual_claude_names_win() {
    let f = Fixture::new();
    f.write(
        &f.claude.join("sessions/1.json"),
        r#"{"sessionId":"claude-a","name":"repo-01","nameSource":"derived"}"#,
    );
    f.write(
        &f.claude.join("sessions/2.json"),
        r#"{"sessionId":"claude-b","name":"Manual name","nameSource":"custom"}"#,
    );
    f.write(
        &f.claude.join("projects/repo/claude-a.jsonl"),
        "{\"sessionId\":\"claude-a\",\"type\":\"ai-title\",\"aiTitle\":\"修复登录重定向\"}\n",
    );
    f.write(
        &f.claude.join("projects/repo/claude-b.jsonl"),
        "{\"sessionId\":\"claude-b\",\"type\":\"ai-title\",\"aiTitle\":\"Generated name\"}\n",
    );
    f.write(&f.codex.join("session_index.jsonl"), "{\"id\":\"codex-a\",\"thread_name\":\"Old\"}\nbroken\n{\"id\":\"codex-a\",\"thread_name\":\"New\"}\n");
    let session_dir = f.kimi.join("session-a");
    std::fs::create_dir(&session_dir).unwrap();
    f.write(
        &session_dir.join("state.json"),
        r#"{"id":"kimi-a","title":"Kimi topic","isCustomTitle":true}"#,
    );
    f.write(
        &f.kimi.join("session_index.jsonl"),
        &format!(
            "{}\n",
            serde_json::json!({"sessionId":"kimi-a","sessionDir":session_dir})
        ),
    );
    let names = f.scan(&mut SessionNameScanner::default());
    assert_eq!(names.get("claude-a").unwrap(), "修复登录重定向");
    assert_eq!(names.get("claude-b").unwrap(), "Manual name");
    assert_eq!(names.get("codex-a").unwrap(), "New");
    assert_eq!(names.get("kimi-a").unwrap(), "Kimi topic");
}

#[test]
fn claude_transcript_titles_are_incremental_and_retry_incomplete_records() {
    let f = Fixture::new();
    f.write(&f.claude.join("sessions/1.json"), r#"{"sessionId":"a"}"#);
    let path = f.claude.join("projects/repo/a.jsonl");
    let initial = "{\"sessionId\":\"a\",\"type\":\"ai-title\",\"aiTitle\":\"Generated\"}\n";
    f.write(&path, initial);
    let mut scanner = SessionNameScanner::default();
    assert_eq!(f.scan(&mut scanner)["a"], "Generated");
    assert_eq!(scanner.transcripts[&path].offset, initial.len() as u64);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    write!(
        file,
        "{{\"sessionId\":\"a\",\"type\":\"custom-title\",\"customTitle\":\"Manual"
    )
    .unwrap();
    assert_eq!(f.scan(&mut scanner)["a"], "Generated");
    writeln!(file, " name\"}}").unwrap();
    assert_eq!(f.scan(&mut scanner)["a"], "Manual name");
    writeln!(
        file,
        "{{\"sessionId\":\"other\",\"type\":\"custom-title\",\"customTitle\":\"Foreign\"}}"
    )
    .unwrap();
    writeln!(
        file,
        "{{\"sessionId\":\"a\",\"type\":\"ai-title\",\"aiTitle\":\"Later generated\"}}"
    )
    .unwrap();
    assert_eq!(f.scan(&mut scanner)["a"], "Manual name");
}

#[test]
fn claude_cache_resets_on_replacement_and_prunes_unregistered_sessions() {
    let f = Fixture::new();
    let registry = f.claude.join("sessions/1.json");
    f.write(&registry, r#"{"sessionId":"a"}"#);
    let path = f.claude.join("projects/repo/a.jsonl");
    f.write(
        &path,
        "{\"sessionId\":\"a\",\"type\":\"custom-title\",\"customTitle\":\"Old\"}\n",
    );
    let mut scanner = SessionNameScanner::default();
    assert_eq!(f.scan(&mut scanner)["a"], "Old");
    let replacement = path.with_extension("new");
    f.write(
        &replacement,
        "{\"sessionId\":\"a\",\"type\":\"ai-title\",\"aiTitle\":\"New longer title\"}\n",
    );
    std::fs::rename(replacement, &path).unwrap();
    assert_eq!(f.scan(&mut scanner)["a"], "New longer title");
    f.write(&path, "");
    assert!(!f.scan(&mut scanner).contains_key("a"));
    std::fs::remove_file(registry).unwrap();
    f.scan(&mut scanner);
    assert!(scanner.transcripts.is_empty());
}

#[test]
fn missing_corrupt_or_mismatched_metadata_does_not_invent_a_title() {
    let f = Fixture::new();
    f.write(&f.claude.join("sessions/1.json"), "not json");
    f.write(
        &f.codex.join("session_index.jsonl"),
        "broken\n{\"id\":\"a\",\"thread_name\":\"  \"}\n",
    );
    f.write(
        &f.claude.join("projects/repo/unregistered.jsonl"),
        "{\"sessionId\":\"unregistered\",\"type\":\"ai-title\",\"aiTitle\":\"Wrong\"}\n",
    );
    f.write(
        &f.kimi.join("state.json"),
        r#"{"id":"other","title":"Wrong session"}"#,
    );
    f.write(
        &f.kimi.join("session_index.jsonl"),
        &format!(
            "{}\n",
            serde_json::json!({"sessionId":"kimi-a","sessionDir":f.kimi})
        ),
    );
    assert!(f.scan(&mut SessionNameScanner::default()).is_empty());
}

#[test]
fn title_cleaning_keeps_unicode_and_pipes_but_removes_terminal_controls() {
    assert_eq!(
        clean_title("\n 修复\t登录 | 跳转\u{7}  "),
        Some("修复 登录 | 跳转".into())
    );
    assert_eq!(clean_title("\u{1b}\n\t"), None);
}
