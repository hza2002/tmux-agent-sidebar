//! Durable, local metering history. All source/database work stays in the usage worker.
mod diagnostics;
mod evidence;
pub(crate) mod store;

use super::{Source, Update, Window, metering};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Health is delivered independently of whether a usable numeric snapshot exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub notice: Option<String>,
    pub details: Vec<String>,
    pub importing: bool,
    pub durable: bool,
    pub available: bool,
    pub last_success: Option<u64>,
}
impl Default for Health {
    fn default() -> Self {
        Self {
            notice: None,
            details: Vec::new(),
            importing: false,
            durable: false,
            available: true,
            last_success: None,
        }
    }
}

pub fn database_path() -> Result<PathBuf, String> {
    xdg_path("XDG_DATA_HOME", ".local/share").map(|p| p.join("tmux-agent-sidebar/usage.sqlite3"))
}
pub fn diagnostic_path() -> Result<PathBuf, String> {
    xdg_path("XDG_STATE_HOME", ".local/state")
        .map(|p| p.join("tmux-agent-sidebar/usage-history.log"))
}
fn xdg_path(variable: &str, fallback: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(path);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(fallback))
        .ok_or_else(|| "HOME must be an absolute directory for usage history".into())
}

const MAX_LINE: usize = 8 * 1024 * 1024;
const MAX_BATCH: usize = 1_000;
const MAX_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
const EXTRACTOR_VERSION: u32 = 1;

/// Bounded raw reader. An oversized line is consumed to its newline without
/// retaining the tail; an unfinished line is retried from its original offset.
fn read_line_bounded(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
) -> std::io::Result<(u64, bool, bool)> {
    line.clear();
    let mut consumed = 0u64;
    let mut oversized = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok((consumed, false, oversized));
        }
        let end = buf.iter().position(|&b| b == b'\n').map(|i| i + 1);
        let n = end.unwrap_or(buf.len());
        consumed += n as u64;
        if line.len().saturating_add(n) > MAX_LINE {
            oversized = true;
        }
        if !oversized {
            line.extend_from_slice(&buf[..n]);
        }
        reader.consume(n);
        if end.is_some() {
            return Ok((consumed, true, oversized));
        }
    }
}

fn stamp(metadata: &fs::Metadata) -> (u64, u64, i64, i64, u64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.len(),
    )
}
fn fingerprint(file: &mut File, start: u64, len: usize) -> Result<u64, String> {
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    // Stable FNV-1a; bounded change detector, not cryptographic identity.
    Ok(bytes.into_iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    }))
}

#[derive(Clone)]
struct SourceFile {
    source: Source,
    path: PathBuf,
    modified: i64,
}
fn inventory(roots: &[(Source, PathBuf)]) -> (Vec<SourceFile>, Vec<String>) {
    fn walk(
        source: Source,
        path: &Path,
        depth: usize,
        out: &mut Vec<SourceFile>,
        errors: &mut Vec<String>,
    ) {
        if depth > 32 {
            errors.push(format!(
                "Source traversal depth exceeded: {}",
                path.display()
            ));
            return;
        }
        let entries = match fs::read_dir(path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!(
                    "Source unreadable: {} ({})",
                    path.display(),
                    e.kind()
                ));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!(
                        "Source directory entry unreadable: {} ({})",
                        path.display(),
                        e.kind()
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!(
                        "Source metadata unavailable: {} ({})",
                        path.display(),
                        e.kind()
                    ));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                walk(source, &path, depth + 1, out, errors);
            } else if meta.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                out.push(SourceFile {
                    source,
                    path,
                    modified: meta.mtime(),
                });
            }
        }
    }
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for (source, root) in roots {
        // An unused optional source is normal; disappearance after import is
        // surfaced by persisted root coverage in the collector.
        if !root.exists() {
            continue;
        }
        walk(*source, root, 0, &mut files, &mut errors);
    }
    files.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    (files, errors)
}

fn scan_batch(store: &store::Store, source: &SourceFile) -> Result<bool, String> {
    let mut file = File::open(&source.path)
        .map_err(|e| format!("Cannot read source {}: {}", source.path.display(), e.kind()))?;
    let current = stamp(&file.metadata().map_err(|e| e.to_string())?);
    let previous = store.checkpoint(&source.path)?;
    let expected = previous.as_ref().map(|p| (p.generation, p.offset));
    let mut checkpoint = previous.clone().unwrap_or(store::FileCheckpoint {
        stamp: current,
        generation: 0,
        offset: 0,
        context: "{}".into(),
        prefix_len: 0,
        prefix_hash: 0,
        boundary_hash: 0,
        extractor_version: EXTRACTOR_VERSION,
    });
    if checkpoint.stamp == current
        && checkpoint.offset == current.4
        && checkpoint.extractor_version == EXTRACTOR_VERSION
        && previous.is_some()
    {
        store.resolve_issues(&source.path.to_string_lossy(), "source_read")?;
        return Ok(true);
    }
    let append = previous.is_some()
        && checkpoint.extractor_version == EXTRACTOR_VERSION
        && current.0 == checkpoint.stamp.0
        && current.1 == checkpoint.stamp.1
        && current.4 >= checkpoint.offset
        && current.4 >= checkpoint.stamp.4
        && (current.4 > checkpoint.stamp.4 || current == checkpoint.stamp)
        && fingerprint(&mut file, 0, checkpoint.prefix_len)? == checkpoint.prefix_hash
        && fingerprint(
            &mut file,
            checkpoint.offset.saturating_sub(512),
            checkpoint.offset.min(512) as usize,
        )? == checkpoint.boundary_hash;
    if !append {
        checkpoint.generation = previous
            .as_ref()
            .map_or(0, |p| p.generation.saturating_add(1));
        checkpoint.offset = 0;
        checkpoint.context = "{}".into();
    }
    let mut context: evidence::Context = serde_json::from_str(&checkpoint.context)
        .map_err(|_| "Stored parser context is invalid; history preserved".to_string())?;
    file.seek(SeekFrom::Start(checkpoint.offset))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let mut bytes = 0;
    let mut complete = false;
    let started = Instant::now();
    loop {
        let offset = checkpoint.offset;
        let (consumed, terminated, oversized) = read_line_bounded(&mut reader, &mut line)
            .map_err(|e| format!("Source read failed: {}", e.kind()))?;
        if !terminated {
            complete = true;
            break;
        }
        checkpoint.offset = checkpoint
            .offset
            .checked_add(consumed)
            .ok_or("Source cursor overflow")?;
        let parsed = if oversized {
            Err("Source line exceeds 8 MiB; history coverage incomplete".to_string())
        } else {
            match std::str::from_utf8(&line) {
                Ok(text) => {
                    evidence::parse_line(source.source, text, &source.path, offset, &mut context)
                }
                Err(_) => Err("Source line is not valid UTF-8".into()),
            }
        };
        match parsed {
            Ok(found) => {
                bytes += found.iter().map(|e| e.canonical.len()).sum::<usize>();
                entries.extend(found);
            }
            Err(message) => issues.push(store::StoreIssue {
                category: "source_parse".into(),
                message,
                path: Some(source.path.to_string_lossy().into_owned()),
                offset: Some(offset),
            }),
        }
        if entries.len() + issues.len() >= MAX_BATCH
            || bytes >= MAX_EVIDENCE_BYTES
            || started.elapsed() >= Duration::from_millis(100)
        {
            break;
        }
    }
    let mut file = reader.into_inner();
    let after = stamp(&file.metadata().map_err(|e| e.to_string())?);
    // Appending during a batch is normal, but a replacement/truncation is not.
    let path_stamp = stamp(&fs::metadata(&source.path).map_err(|e| e.to_string())?);
    if after.0 != current.0
        || after.1 != current.1
        || path_stamp.0 != current.0
        || path_stamp.1 != current.1
        || after.4 < current.4
        || (after.4 == current.4 && after != current)
    {
        return Err("Source changed during parsing; batch discarded and will retry".into());
    }
    checkpoint.stamp = current;
    checkpoint.extractor_version = EXTRACTOR_VERSION;
    checkpoint.context = serde_json::to_string(&context).map_err(|e| e.to_string())?;
    checkpoint.prefix_len = checkpoint.offset.min(512) as usize;
    checkpoint.prefix_hash = fingerprint(&mut file, 0, checkpoint.prefix_len)?;
    checkpoint.boundary_hash = fingerprint(
        &mut file,
        checkpoint.offset.saturating_sub(512),
        checkpoint.offset.min(512) as usize,
    )?;
    store.apply_batch(&source.path, expected, &entries, &checkpoint, &issues)?;
    store.resolve_issues(&source.path.to_string_lossy(), "source_read")?;
    Ok(complete || checkpoint.offset >= current.4)
}

fn snapshot(
    store: &store::Store,
    now: i64,
    cycle: u64,
    account: Option<&str>,
    health: Health,
) -> Result<Update, String> {
    let mut spends = std::array::from_fn(|_| super::Spend::default());
    let mut costs = [0u128; 4];
    let incomplete = health.notice.is_some() || health.importing;
    let mut codex_all = metering::empty_snapshot(now, cycle, incomplete);
    let mut codex_account = metering::empty_snapshot(now, cycle, incomplete);
    let start = Window::ALL
        .iter()
        .map(|w| w.start(now))
        .min()
        .unwrap_or(now)
        .min(codex_all.cycle.as_ref().map_or(now, |s| s.start));
    store.visit_records(start, now, |r| {
        if let Some(meta) = r.codex.as_ref() {
            metering::add_record(&mut codex_all, &r);
            if let Some(id) = account
                && meta.account.as_deref() == Some(id)
            {
                metering::add_record(&mut codex_account, &r);
            }
        } else {
            for w in Window::ALL {
                let i = w.code() as usize;
                if r.ts >= w.start(now) {
                    costs[i] = costs[i].saturating_add(super::add_record(&mut spends[i], &r));
                }
            }
        }
    })?;
    for (s, cost) in spends.iter_mut().zip(costs) {
        s.cost_cny = cost as f64 / 1_000_000_000_000.0;
    }
    Ok(Update {
        history: health,
        spends,
        codex_all,
        codex_account,
        codex_account_id: account.map(str::to_owned),
    })
}

fn unavailable(health: Health) -> Update {
    Update {
        history: health,
        spends: Default::default(),
        codex_all: Default::default(),
        codex_account: Default::default(),
        codex_account_id: None,
    }
}

fn read_health(
    db: &store::Store,
    roots_json: &str,
    owner: bool,
    log: &Result<PathBuf, String>,
) -> Result<Health, String> {
    let mut health = Health {
        durable: true,
        available: true,
        ..Health::default()
    };
    if !owner && db.get_meta("roots")?.as_deref() != Some(roots_json) {
        health.notice = Some("用量采集目录不一致".into());
        health.details.push("Another sidebar collects different source roots; this view does not establish coverage of your configured logs.".into());
    }
    health.importing = db.get_meta("importing")?.as_deref() == Some("1")
        || db.get_meta("inventory_complete")?.as_deref() != Some("1");
    let issues = db.unresolved_issues()?;
    let inventory_errors: Vec<String> = db
        .get_meta("inventory_errors")?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !issues.is_empty() || !inventory_errors.is_empty() {
        health.notice = Some("用量历史不完整".into());
        health.details.extend(inventory_errors.into_iter().take(10));
        health.details.extend(issues.iter().take(10).map(|i| {
            format!(
                "{}: {} ({} @ {})",
                i["category"].as_str().unwrap_or("usage"),
                i["message"].as_str().unwrap_or("source error"),
                i["path"].as_str().unwrap_or("store"),
                i["offset"]
            )
        }));
    }
    health.last_success = db.get_meta("last_success")?.and_then(|s| s.parse().ok());
    health.available = !health.importing || health.last_success.is_some();
    if !health.importing {
        let parents = db.unresolved_parent_count()?;
        if parents > 0 {
            health.notice = Some("用量历史不完整".into());
            health.details.push(format!("{parents} fork replay record(s) have no available parent evidence; totals may contain duplicates."));
        }
        // Age the retained evidence, not the worker or the aggregate count.
        // A later response must not postpone warnings about an older one.
        let grace = super::REFRESH_INTERVAL.as_secs() * 3;
        let cutoff = crate::time::now_epoch_secs().saturating_sub(grace) as i64;
        let pending = db.pending_response_count_before(cutoff)?;
        if pending > 0 {
            health.notice = Some("用量历史不完整".into());
            health.details.push(format!("{pending} metering response(s) lack matching count/compaction evidence; excluded from totals. See usage-history status."));
        }
    }
    if let Ok(log) = &log {
        health
            .details
            .push(format!("Diagnostics: {}", log.display()));
    }
    health
        .details
        .push("Inspect: tmux-agent-sidebar usage-history status".into());
    Ok(health)
}

fn merge_inventory(queue: &mut VecDeque<SourceFile>, files: Vec<SourceFile>) {
    let pending: std::collections::HashSet<_> = queue.iter().map(|f| f.path.clone()).collect();
    let fresh: Vec<_> = files
        .into_iter()
        .filter(|f| !pending.contains(&f.path))
        .collect();
    // Newly discovered or already-completed active files are visited ahead of
    // the existing round-robin backlog. Pending files retain their progress.
    for file in fresh.into_iter().rev() {
        queue.push_front(file);
    }
}

pub(super) fn run(
    tx: &Sender<Update>,
    force: &AtomicBool,
    cycle: &std::sync::Mutex<super::QuotaCycle>,
) {
    let path = database_path();
    let log = diagnostic_path();
    let mut diagnostics = diagnostics::Diagnostics::default();
    let roots = super::roots();
    let roots_json = serde_json::to_string(
        &roots
            .iter()
            .map(|(_, p)| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let mut store: Option<store::Store> = None;
    let mut owner = false;
    let mut queue = VecDeque::new();
    let mut scan_errors = Vec::new();
    let mut last_inventory = Instant::now() - super::REFRESH_INTERVAL;
    let mut last_publish = Instant::now() - Duration::from_secs(1);
    let mut pending_refresh = false;
    let mut account = None;
    let mut last_account_read = Instant::now() - super::REFRESH_INTERVAL;
    loop {
        pending_refresh |= force.swap(false, Ordering::Relaxed);
        let now = crate::time::now_epoch_secs();
        let mut health = Health {
            available: false,
            ..Health::default()
        };
        let opened = if store.is_none() || !owner {
            match &path {
                Ok(path) => match store::Store::open_writer(path) {
                    Ok(db) => {
                        owner = true;
                        store = Some(db);
                        last_inventory = Instant::now() - super::REFRESH_INTERVAL;
                        Ok(())
                    }
                    Err(error) if error.starts_with("collector busy") => {
                        owner = false;
                        match store::Store::open_reader(path) {
                            Ok(db) => {
                                store = Some(db);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                },
                Err(e) => Err(e.clone()),
            }
        } else {
            Ok(())
        };
        let operation = (|| -> Result<Option<Update>, String> {
            opened?;
            if last_account_read.elapsed() >= super::REFRESH_INTERVAL || pending_refresh {
                account = super::attribution::daemon_account();
                last_account_read = Instant::now();
            }
            // Pair the numerator only with a boundary fetched for this account.
            // A stale/error-retained quota from the previous login stays excluded.
            let serving_cycle = cycle
                .lock()
                .ok()
                .map_or(0, |cycle| cycle.for_account(account.as_deref()));
            let db = store.as_ref().ok_or("Usage history unavailable")?;
            if owner {
                db.import_attribution(now)?;
            }
            health = read_health(db, &roots_json, owner, &log)?;
            // Publish persisted windows before doing a potentially large inventory.
            if last_publish.elapsed() >= Duration::from_secs(1)
                && last_inventory.elapsed() >= super::REFRESH_INTERVAL
                && queue.is_empty()
            {
                let warm = health.clone();
                if tx
                    .send(snapshot(
                        db,
                        now as i64,
                        serving_cycle,
                        account.as_deref(),
                        warm,
                    )?)
                    .is_err()
                {
                    return Err("usage receiver closed".into());
                }
                last_publish = Instant::now();
            }
            if owner {
                db.set_meta("roots", &roots_json)?;
                db.set_meta("heartbeat", &now.to_string())?;
                if last_inventory.elapsed() >= super::REFRESH_INTERVAL
                    || (last_inventory.elapsed() >= Duration::from_secs(1) && pending_refresh)
                {
                    pending_refresh = false;
                    let (files, errors) = inventory(&roots);
                    merge_inventory(&mut queue, files);
                    scan_errors = errors;
                    last_inventory = Instant::now();
                    for (_, root) in &roots {
                        let key = format!("root_seen:{}", root.display());
                        if root.is_dir() {
                            db.set_meta(&key, "1")?;
                        } else if db.get_meta(&key)?.is_some() {
                            scan_errors.push(format!(
                                "Previously imported root unavailable: {}",
                                root.display()
                            ));
                        }
                    }
                    db.set_meta("importing", "1")?;
                }
                let began = Instant::now();
                while let Some(file) = queue.pop_front() {
                    match scan_batch(db, &file) {
                        Ok(false) => queue.push_back(file),
                        Ok(true) => {}
                        Err(message) => {
                            db.record_issue(&store::StoreIssue {
                                category: "source_read".into(),
                                message,
                                path: Some(file.path.to_string_lossy().into_owned()),
                                offset: None,
                            })?;
                        }
                    }
                    if began.elapsed() >= Duration::from_millis(200) {
                        break;
                    }
                }
                if queue.is_empty() {
                    db.set_meta("importing", "0")?;
                    db.set_meta("inventory_complete", "1")?;
                }
                db.set_meta(
                    "inventory_errors",
                    &serde_json::to_string(&scan_errors).unwrap_or_default(),
                )?;
            }
            health = read_health(db, &roots_json, owner, &log)?;
            if last_publish.elapsed() >= Duration::from_secs(1) || queue.is_empty() {
                Ok(Some(snapshot(
                    db,
                    now as i64,
                    serving_cycle,
                    account.as_deref(),
                    health.clone(),
                )?))
            } else {
                Ok(None)
            }
        })();
        let mut update = match operation {
            Ok(update) => update,
            Err(error) => {
                if error == "usage receiver closed" {
                    return;
                }
                health.available = false;
                health.durable = false;
                health.notice = Some("用量历史保存失败".into());
                health.details = vec![
                    error,
                    "History has been preserved. Inspect: tmux-agent-sidebar usage-history status"
                        .into(),
                ];
                // Keep the last usable UI snapshot; never replace it with zeros.
                store = None;
                owner = false;
                queue.clear();
                Some(unavailable(health.clone()))
            }
        };
        if let Some(ref mut update) = update {
            if let Ok(log) = &log {
                if let Err(error) = diagnostics.transition(
                    log,
                    update.history.notice.as_deref(),
                    &update.history.details,
                ) {
                    update
                        .history
                        .notice
                        .get_or_insert_with(|| "用量诊断日志写入失败".into());
                    update.history.details.push(error.clone());
                    eprintln!("usage history: {error}");
                }
            } else if let Err(error) = &log {
                update
                    .history
                    .notice
                    .get_or_insert_with(|| "用量诊断日志不可用".into());
                update.history.details.push(error.clone());
            }
        }
        if let Some(update) = update {
            if tx.send(update).is_err() {
                return;
            }
            last_publish = Instant::now();
        }
        if queue.is_empty() || store.is_none() || !owner {
            let interval = if owner && pending_refresh {
                Duration::from_secs(1).saturating_sub(last_inventory.elapsed())
            } else {
                pending_refresh = false;
                super::REFRESH_INTERVAL
            };
            super::sleep_until_due(interval, force);
        } else {
            std::thread::yield_now();
        }
    }
}

/// Maintenance is explicit; ordinary status/backup/export never starts a collector.
pub fn command(args: &[String]) -> Result<String, String> {
    let path = database_path()?;
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["status"] => {
            if !path.exists() {
                return Ok(serde_json::json!({
                    "database": path,
                    "state": "not initialized",
                    "note": "Collection runs only while an enabled sidebar is running."
                }).to_string());
            }
            let db = store::Store::open_reader(&path)?;
            serde_json::to_string_pretty(&db.status()?).map_err(|e| e.to_string())
        }
        ["backup", destination] => {
            let db = store::Store::open_reader(&path)?;
            db.backup(Path::new(destination))?;
            Ok(format!("Usage history backup created: {destination}"))
        }
        ["export", destination] => {
            let db = store::Store::open_reader(&path)?;
            db.export(Path::new(destination))?;
            Ok(format!("Usage history evidence exported: {destination}"))
        }
        ["purge", "--all", "--confirm"] => {
            if !path.exists() {
                return Err("Usage history has not been initialized; nothing to purge".into());
            }
            let mut db = store::Store::open_writer(&path)?;
            let backup = db.purge()?;
            Ok(format!(
                "Usage history cleared. Backup: {}\nExisting source logs will be imported again when collection resumes.",
                backup.display()
            ))
        }
        _ => Err("usage: tmux-agent-sidebar usage-history status | backup <new-path> | export <new-path> | purge --all --confirm (stop collectors first)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn setup() -> (tempfile::TempDir, store::Store, SourceFile) {
        let temp = tempfile::tempdir().unwrap();
        let db = store::Store::open_writer(&temp.path().join("data/usage.sqlite3")).unwrap();
        let path = temp.path().join("session.jsonl");
        fs::write(&path,concat!("{\"type\":\"session_meta\",\"payload\":{\"id\":\"s\"}}\n","{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6.1-sol\",\"turn_id\":\"t\"}}\n")).unwrap();
        (
            temp,
            db,
            SourceFile {
                source: Source::Codex,
                path,
                modified: 0,
            },
        )
    }
    fn event(total: u64) -> String {
        format!(
            "{{\"timestamp\":\"2026-10-26T01:00:00.123Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":100,\"cached_input_tokens\":60,\"output_tokens\":10}},\"total_token_usage\":{{\"input_tokens\":{total},\"output_tokens\":10,\"total_tokens\":{}}}}}}}}}\n",
            total + 10
        )
    }
    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    #[test]
    fn durable_import_restarts_resumes_partial_tail_and_survives_deleted_source() {
        let (temp, db, file) = setup();
        let value = event(100);
        let cut = value.len() / 2;
        append(&file.path, &value[..cut]);
        assert!(scan_batch(&db, &file).unwrap());
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 0);
        let path = temp.path().join("data/usage.sqlite3");
        drop(db);
        append(&file.path, &value[cut..]);
        let db = store::Store::open_writer(&path).unwrap();
        assert!(scan_batch(&db, &file).unwrap());
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 1);
        assert!(scan_batch(&db, &file).unwrap());
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 1);
        fs::remove_file(&file.path).unwrap();
        drop(db);
        assert_eq!(
            store::Store::open_reader(&path)
                .unwrap()
                .records(0, i64::MAX)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn all_history_inventory_ignores_old_mtime_and_skips_symlinks() {
        let (temp, _, file) = setup();
        File::open(&file.path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        let link = temp.path().join("copy.jsonl");
        std::os::unix::fs::symlink(&file.path, link).unwrap();
        let (files, errors) = inventory(&[(Source::Codex, temp.path().to_path_buf())]);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 1);
    }
    #[test]
    fn bounded_reader_consumes_oversize_without_retaining_tail() {
        let mut bytes = vec![b'x'; MAX_LINE + 100];
        bytes.extend_from_slice(b"\n{}\n");
        let mut reader = BufReader::new(bytes.as_slice());
        let mut line = Vec::new();
        let (n, complete, oversized) = read_line_bounded(&mut reader, &mut line).unwrap();
        assert_eq!(n, MAX_LINE as u64 + 101);
        assert!(complete && oversized);
        assert!(line.len() <= MAX_LINE);
        assert_eq!(
            read_line_bounded(&mut reader, &mut line).unwrap(),
            (3, true, false)
        );
        assert_eq!(line, b"{}\n");
    }
    #[test]
    fn rewritten_truncated_and_replaced_sources_restart_without_erasing_history() {
        let (temp, db, file) = setup();
        let header = fs::read_to_string(&file.path).unwrap();
        append(&file.path, &event(100));
        while !scan_batch(&db, &file).unwrap() {}
        let initial = db.checkpoint(&file.path).unwrap().unwrap();
        fs::write(&file.path, format!("{header}{}", event(200))).unwrap();
        File::open(&file.path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        while !scan_batch(&db, &file).unwrap() {}
        let rewritten = db.checkpoint(&file.path).unwrap().unwrap();
        assert_eq!(rewritten.generation, initial.generation + 1);
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 1);
        assert!(
            db.unresolved_issues()
                .unwrap()
                .iter()
                .any(|i| i["category"] == "source_revision")
        );

        fs::write(&file.path, &header).unwrap();
        while !scan_batch(&db, &file).unwrap() {}
        let truncated = db.checkpoint(&file.path).unwrap().unwrap();
        assert_eq!(truncated.generation, rewritten.generation + 1);
        assert_eq!(truncated.offset, header.len() as u64);
        append(&file.path, &event(100));
        while !scan_batch(&db, &file).unwrap() {}
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 1);

        let replacement = temp.path().join("replacement.jsonl");
        fs::write(
            &replacement,
            format!("{}{}", header.replace("\"s\"", "\"q\""), event(100)),
        )
        .unwrap();
        fs::rename(replacement, &file.path).unwrap();
        while !scan_batch(&db, &file).unwrap() {}
        let replaced = db.checkpoint(&file.path).unwrap().unwrap();
        assert_eq!(replaced.generation, truncated.generation + 1);
        assert_ne!(replaced.stamp.1, truncated.stamp.1);
        assert_eq!(db.records(0, i64::MAX).unwrap().len(), 2);
    }

    #[test]
    fn oversized_source_line_is_persistently_visible_until_source_is_repaired() {
        let (_temp, db, file) = setup();
        let header = fs::read_to_string(&file.path).unwrap();
        append(&file.path, &"x".repeat(MAX_LINE + 1));
        append(&file.path, "\n");
        while !scan_batch(&db, &file).unwrap() {}
        db.set_meta("inventory_complete", "1").unwrap();
        let log = Ok(PathBuf::from("/tmp/log"));
        assert_eq!(db.issue_count().unwrap(), 1);
        assert!(read_health(&db, "[]", true, &log).unwrap().notice.is_some());
        assert!(scan_batch(&db, &file).unwrap());
        assert_eq!(db.issue_count().unwrap(), 1);
        fs::write(&file.path, header).unwrap();
        while !scan_batch(&db, &file).unwrap() {}
        assert_eq!(db.issue_count().unwrap(), 0);
        assert!(read_health(&db, "[]", true, &log).unwrap().notice.is_none());
    }

    #[test]
    fn unavailable_history_keeps_last_snapshot_and_does_not_create_zero() {
        let mut state = super::super::UsageState::default();
        let health = Health {
            available: false,
            notice: Some("failure".into()),
            ..Health::default()
        };
        state.apply(unavailable(health.clone()));
        assert!(!state.received);
        state.apply(Update {
            history: Health::default(),
            spends: std::array::from_fn(|_| super::super::Spend {
                cost_cny: 42.0,
                ..Default::default()
            }),
            codex_all: Default::default(),
            codex_account: Default::default(),
            codex_account_id: None,
        });
        state.apply(unavailable(health));
        assert_eq!(state.snapshot.unwrap().cost_cny, 42.0);
        assert!(state.history.notice.is_some());
    }
    #[test]
    fn persisted_query_reprices_without_source_files() {
        let (_temp, db, file) = setup();
        append(&file.path, &event(100));
        scan_batch(&db, &file).unwrap();
        fs::remove_file(&file.path).unwrap();
        let now = super::super::parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        let update = snapshot(&db, now, 0, None, Health::default()).unwrap();
        assert_eq!(update.codex_all.recent[0].total.tokens.total(), 110);
        assert!(update.codex_all.recent[0].total.cost > 0);
    }
    #[test]
    fn account_scope_splits_records_by_creator_account() {
        let temp = tempfile::tempdir().unwrap();
        let db = store::Store::open_writer(&temp.path().join("data/usage.sqlite3")).unwrap();
        let now = super::super::parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        for (name, account, total) in [("a", "acct-a", 100), ("b", "acct-b", 300)] {
            let path = temp.path().join(format!("{name}.jsonl"));
            fs::write(
                &path,
                format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{name}\",\"creator_account_id\":\"{account}\"}}}}\n\
                     {{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-6.1-sol\",\"turn_id\":\"{name}-t\"}}}}\n{}",
                    event(total)
                ),
            )
            .unwrap();
            let source = SourceFile {
                source: Source::Codex,
                path,
                modified: 0,
            };
            assert!(scan_batch(&db, &source).unwrap());
        }
        // An unattributed record belongs to the all-accounts row only.
        let path = temp.path().join("c.jsonl");
        fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"c\"}}}}\n\
                 {{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-6.1-sol\",\"turn_id\":\"c-t\"}}}}\n{}",
                event(500)
            ),
        )
        .unwrap();
        let source = SourceFile {
            source: Source::Codex,
            path,
            modified: 0,
        };
        assert!(scan_batch(&db, &source).unwrap());
        let update = snapshot(&db, now, 0, Some("acct-a"), Health::default()).unwrap();
        assert_eq!(update.codex_all.recent[0].total.tokens.total(), 330);
        assert_eq!(update.codex_account.recent[0].total.tokens.total(), 110);
        let update = snapshot(&db, now, 0, None, Health::default()).unwrap();
        assert_eq!(update.codex_all.recent[0].total.tokens.total(), 330);
        assert_eq!(update.codex_account.recent[0].total.tokens.total(), 0);
    }

    #[test]
    fn unchanged_source_resolves_transient_read_issue() {
        let (_temp, db, file) = setup();
        append(&file.path, &event(100));
        scan_batch(&db, &file).unwrap();
        db.record_issue(&store::StoreIssue {
            category: "source_read".into(),
            message: "temporarily unreadable".into(),
            path: Some(file.path.to_string_lossy().into_owned()),
            offset: None,
        })
        .unwrap();
        assert_eq!(db.issue_count().unwrap(), 1);
        scan_batch(&db, &file).unwrap();
        assert_eq!(db.issue_count().unwrap(), 0);
    }
    #[test]
    fn warm_health_preserves_errors_and_does_not_confuse_owner_with_importing() {
        let (_temp, db, _file) = setup();
        db.set_meta("inventory_complete", "1").unwrap();
        db.record_issue(&store::StoreIssue {
            category: "disk".into(),
            message: "failure".into(),
            path: None,
            offset: None,
        })
        .unwrap();
        let health = read_health(&db, "[]", true, &Ok(PathBuf::from("/tmp/log"))).unwrap();
        assert!(health.notice.is_some());
        assert!(!health.importing);
    }
    #[test]
    fn new_inventory_merges_active_files_without_discarding_backfill() {
        let file = |path: &str| SourceFile {
            source: Source::Codex,
            path: path.into(),
            modified: 0,
        };
        let mut queue = VecDeque::from([file("old")]);
        merge_inventory(&mut queue, vec![file("active"), file("old")]);
        assert_eq!(
            queue
                .iter()
                .map(|f| f.path.to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["active", "old"]
        );
    }
    #[test]
    fn uninitialized_store_does_not_publish_complete_zero_usage() {
        let (_temp, db, _file) = setup();
        let health = read_health(&db, "[]", true, &Ok(PathBuf::from("/tmp/log"))).unwrap();
        assert!(health.importing);
        assert!(!health.available);
    }
}

#[cfg(test)]
mod benchmarks {
    use super::*;
    fn peak_rss_bytes() -> u64 {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return 0;
        }
        let rss = unsafe { usage.assume_init() }.ru_maxrss as u64;
        if cfg!(target_os = "macos") {
            rss
        } else {
            rss.saturating_mul(1024)
        }
    }
    #[test]
    #[ignore = "explicit synthetic scale benchmark; creates only a temporary store"]
    fn synthetic_history_scale_benchmark() {
        let n: usize = std::env::var("TAS_USAGE_BENCH_EVENTS")
            .expect("explicit size required")
            .parse()
            .unwrap();
        assert!(n <= 1_500_000);
        let baseline_rss = peak_rss_bytes();
        let temp = tempfile::tempdir().unwrap();
        let dbpath = temp.path().join("data/usage.sqlite3");
        let db = store::Store::open_writer(&dbpath).unwrap();
        let source = Path::new("/synthetic/usage.jsonl");
        let now = crate::time::now_epoch_secs();
        let mut cp = store::FileCheckpoint {
            stamp: (1, 1, 0, 0, 0),
            generation: 0,
            offset: 0,
            context: "{}".into(),
            prefix_len: 0,
            prefix_hash: 0,
            boundary_hash: 0,
            extractor_version: EXTRACTOR_VERSION,
        };
        let mut ctx = evidence::Context::default();
        for line in [
            r#"{"type":"session_meta","payload":{"id":"synthetic-session","cwd":"/synthetic/project"}}"#,
            r#"{"type":"turn_context","payload":{"model":"gpt-6.1-sol","turn_id":"synthetic-turn"}}"#,
        ] {
            evidence::parse_line(Source::Codex, line, source, 0, &mut ctx).unwrap();
        }
        let ts = metering::format_time(now as i64 - 60).replace(' ', "T") + ":00Z";
        let import_start = Instant::now();
        for start in (0..n).step_by(1000) {
            let mut records = Vec::new();
            for i in start..(start + 1000).min(n) {
                let line=serde_json::json!({"timestamp":ts,"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":60,"output_tokens":10},"total_token_usage":{"input_tokens":(i+1)*100,"cached_input_tokens":(i+1)*60,"output_tokens":(i+1)*10,"total_tokens":(i+1)*110}}}}).to_string();
                records.extend(
                    evidence::parse_line(Source::Codex, &line, source, i as u64, &mut ctx).unwrap(),
                );
            }
            let expected = if start == 0 {
                None
            } else {
                Some((0, cp.offset))
            };
            cp.offset = (start + records.len()) as u64;
            db.apply_batch(source, expected, &records, &cp, &[])
                .unwrap();
        }
        let import_ms = import_start.elapsed().as_millis();
        let mut times = Vec::new();
        for _ in 0..20 {
            let t = Instant::now();
            let update = snapshot(&db, now as i64, 0, None, Health::default()).unwrap();
            assert_eq!(
                update.codex_all.recent[0].total.tokens.total(),
                n as u64 * 110
            );
            times.push(t.elapsed().as_micros());
        }
        times.sort();
        println!(
            "{}",
            serde_json::json!({"synthetic_events":n,"import_ms":import_ms,"query_p50_us":times[10],"query_p95_us":times[19],"peak_rss_bytes":peak_rss_bytes(),"additional_peak_rss_bytes":peak_rss_bytes().saturating_sub(baseline_rss),"db_bytes":fs::metadata(dbpath).unwrap().len()})
        );
    }
    /// Explicit opt-in, read-only source benchmark. The derived store is private
    /// and ephemeral. Only aggregate measurements are printed.
    #[test]
    #[ignore = "requires explicit TAS_USAGE_BENCH_ROOTS JSON paths; source-read only"]
    fn live_history_import_benchmark() {
        let raw = std::env::var("TAS_USAGE_BENCH_ROOTS").expect("explicit source roots required");
        let paths: Vec<(String, PathBuf)> = serde_json::from_str(&raw).unwrap();
        let roots: Vec<_> = paths
            .into_iter()
            .map(|(s, p)| {
                (
                    if s == "codex" {
                        Source::Codex
                    } else {
                        assert_eq!(s, "claude");
                        Source::Claude
                    },
                    p,
                )
            })
            .collect();
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("data/usage.sqlite3");
        let db = store::Store::open_writer(&db_path).unwrap();
        let baseline_rss = peak_rss_bytes();
        let started = Instant::now();
        let (files, errors) = inventory(&roots);
        assert!(errors.is_empty());
        let n = files.len();
        let source_bytes: u64 = files
            .iter()
            .map(|f| fs::metadata(&f.path).unwrap().len())
            .sum();
        for file in &files {
            while !scan_batch(&db, file).unwrap() {}
        }
        let import_ms = started.elapsed().as_millis();
        let now = crate::time::now_epoch_secs() as i64;
        let mut queries = Vec::new();
        for _ in 0..20 {
            let t = Instant::now();
            let _ = snapshot(&db, now, 0, None, Health::default()).unwrap();
            queries.push(t.elapsed().as_micros());
        }
        let first_query_us = queries[0];
        queries.sort();
        db.set_meta("inventory_complete", "1").unwrap();
        let health_started = Instant::now();
        let _health =
            read_health(&db, "[]", true, &Ok(temp.path().join("diagnostic.log"))).unwrap();
        let health_us = health_started.elapsed().as_micros();
        let idle = Instant::now();
        for f in &files {
            assert!(scan_batch(&db, f).unwrap());
        }
        let idle_ms = idle.elapsed().as_millis();
        let status = db.status().unwrap();
        println!(
            "{}",
            serde_json::json!({"files":n,"source_bytes":source_bytes,"import_ms":import_ms,"idle_scan_ms":idle_ms,"first_query_us":first_query_us,"health_us":health_us,"query_p50_us":queries[10],"query_p95_us":queries[18],"query_max_us":queries[19],"events":status["events"],"observations":status["observations"],"storage":status["storage"],"pending_response_count":status["pending_response_count"],"unresolved_parent_count":status["unresolved_parent_count"],"issues":db.issue_count().unwrap(),"db_bytes":fs::metadata(&db_path).unwrap().len(),"wal_bytes":fs::metadata(db_path.with_extension("sqlite3-wal")).map(|m|m.len()).unwrap_or(0),"sqlite":rusqlite::version(),"peak_rss_bytes":peak_rss_bytes(),"additional_peak_rss_bytes":peak_rss_bytes().saturating_sub(baseline_rss)})
        );
    }
}
