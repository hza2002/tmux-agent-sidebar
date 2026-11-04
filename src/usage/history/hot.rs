//! Bounded read-only quota tailing, shared by the history writer and followers.
use super::*;

pub(super) const MAX_FILES: usize = 16;
const HEAD_BYTES: u64 = 64 * 1024;
const READ_BYTES: u64 = 256 * 1024;
const BOOTSTRAP_BYTES: u64 = 4 * 1024 * 1024;
const RECENT: u64 = 15 * 60;

#[derive(Default)]
pub(super) struct HotFiles {
    files: Vec<Tail>,
}
struct Tail {
    source: SourceFile,
    cursor: Option<Cursor>,
}
struct Cursor {
    stamp: (u64, u64, i64, i64, u64),
    offset: u64,
    prefix_len: usize,
    prefix_hash: u64,
    boundary_hash: u64,
    context: evidence::Context,
    skipping_line: bool,
    waiting_for_append: bool,
}

impl HotFiles {
    pub(super) fn discover(&mut self, files: &[SourceFile], now: u64) {
        // Keep known recent files even when idle; discovery refreshes this bounded set.
        let selected: Vec<_> = files
            .iter()
            .filter(|f| matches!(f.source, Source::Codex))
            .filter(|f| {
                now.saturating_sub(f.modified.max(0) as u64) <= RECENT
                    || self.files.iter().any(|old| old.source.path == f.path)
            })
            .take(MAX_FILES)
            .cloned()
            .collect();
        let mut previous = std::mem::take(&mut self.files);
        self.files = selected
            .into_iter()
            .map(|source| {
                if let Some(index) = previous
                    .iter()
                    .position(|old| old.source.path == source.path)
                {
                    let mut tail = previous.swap_remove(index);
                    tail.source = source;
                    tail
                } else {
                    Tail {
                        source,
                        cursor: None,
                    }
                }
            })
            .collect();
    }

    pub(super) fn poll(&mut self, quota: &mut QuotaCandidates) -> Vec<SourceFile> {
        let mut changed = Vec::new();
        // Startup/replacement reads share one 4 MiB budget across all files.
        let mut bootstrap_remaining = BOOTSTRAP_BYTES;
        for tail in &mut self.files {
            match tail.read_bounded(&mut bootstrap_remaining) {
                Ok(Some(candidates)) => {
                    quota.accept(candidates, crate::time::now_epoch_millis());
                    changed.push(tail.source.clone());
                }
                Ok(None) => {}
                Err(_) => {
                    tail.cursor = None;
                }
            }
        }
        // An unreadable large file must not consume every startup budget first.
        if !self.files.is_empty() {
            self.files.rotate_left(1);
        }
        changed
    }
}

impl Tail {
    #[cfg(test)]
    fn read(&mut self) -> Result<Option<Vec<evidence::QuotaCandidate>>, String> {
        let mut remaining = BOOTSTRAP_BYTES;
        self.read_bounded(&mut remaining)
    }

    fn read_bounded(
        &mut self,
        bootstrap_remaining: &mut u64,
    ) -> Result<Option<Vec<evidence::QuotaCandidate>>, String> {
        let mut file = File::open(&self.source.path).map_err(|e| e.to_string())?;
        let current = stamp(&file.metadata().map_err(|e| e.to_string())?);
        if self.cursor.as_ref().is_some_and(|cursor| {
            cursor.stamp == current && (cursor.offset == current.4 || cursor.waiting_for_append)
        }) {
            return Ok(None);
        }
        let append = if let Some(cursor) = &self.cursor {
            current.0 == cursor.stamp.0
                && current.1 == cursor.stamp.1
                && current.4 >= cursor.offset
                && current.4 >= cursor.stamp.4
                && (current.4 > cursor.stamp.4 || current == cursor.stamp)
                && fingerprint(&mut file, 0, cursor.prefix_len)? == cursor.prefix_hash
                && fingerprint(
                    &mut file,
                    cursor.offset.saturating_sub(512),
                    cursor.offset.min(512) as usize,
                )? == cursor.boundary_hash
        } else {
            false
        };
        let read_limit = if append { READ_BYTES } else { BOOTSTRAP_BYTES };
        if !append {
            let cost = current.4.min(BOOTSTRAP_BYTES);
            if cost > *bootstrap_remaining {
                return Ok(None);
            }
            *bootstrap_remaining -= cost;
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            let mut head = String::new();
            BufReader::new((&mut file).take(HEAD_BYTES))
                .read_line(&mut head)
                .map_err(|e| e.to_string())?;
            if !head.ends_with('\n') {
                return Err("Incomplete quota header".into());
            }
            let context = evidence::quota_header(&head).ok_or("Invalid quota header")?;
            let mut offset = current.4.saturating_sub(BOOTSTRAP_BYTES);
            let mut skipping_line = false;
            if offset > 0 {
                file.seek(SeekFrom::Start(offset))
                    .map_err(|e| e.to_string())?;
                // Discard a possibly partial first tail line. No turn from the head is reused.
                let mut reader = BufReader::new((&mut file).take(BOOTSTRAP_BYTES));
                let mut discard = Vec::new();
                offset += reader
                    .read_until(b'\n', &mut discard)
                    .map_err(|e| e.to_string())? as u64;
                skipping_line = !discard.ends_with(b"\n");
            }
            self.cursor = Some(Cursor {
                stamp: current,
                offset,
                prefix_len: 0,
                prefix_hash: 0,
                boundary_hash: 0,
                context,
                skipping_line,
                waiting_for_append: false,
            });
        }
        let cursor = self.cursor.as_mut().expect("initialized above");
        file.seek(SeekFrom::Start(cursor.offset))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(
            (&mut file).take(read_limit.min(current.4.saturating_sub(cursor.offset))),
        );
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let mut candidates = Vec::new();
        cursor.waiting_for_append = false;
        // Partial normal records are retried. Oversized records are skipped across
        // bounded reads so one large line cannot permanently block later quota.
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            let terminated = line.ends_with(b"\n");
            if cursor.skipping_line {
                cursor.offset += line.len() as u64;
                cursor.skipping_line = !terminated;
                continue;
            }
            if !terminated {
                if line.len() == read_limit as usize {
                    cursor.offset += line.len() as u64;
                    cursor.skipping_line = true;
                    evidence::clear_quota_turn(&mut cursor.context);
                } else {
                    cursor.waiting_for_append = cursor.offset + line.len() as u64 == current.4;
                }
                break;
            }
            match std::str::from_utf8(line) {
                Ok(text) => {
                    if let Some(candidate) = evidence::quota_line(text, &mut cursor.context) {
                        candidates.push(candidate);
                    }
                }
                Err(_) => evidence::clear_quota_turn(&mut cursor.context),
            }
            cursor.offset += line.len() as u64;
        }
        let after = stamp(&file.metadata().map_err(|e| e.to_string())?);
        let path_stamp = stamp(&fs::metadata(&self.source.path).map_err(|e| e.to_string())?);
        if after.0 != current.0
            || after.1 != current.1
            || after.4 < current.4
            || path_stamp.0 != current.0
            || path_stamp.1 != current.1
            || (after.4 == current.4 && after != current)
        {
            return Err("Quota source changed during read".into());
        }
        cursor.stamp = current;
        cursor.prefix_len = cursor.offset.min(512) as usize;
        cursor.prefix_hash = fingerprint(&mut file, 0, cursor.prefix_len)?;
        cursor.boundary_hash = fingerprint(
            &mut file,
            cursor.offset.saturating_sub(512),
            cursor.offset.min(512) as usize,
        )?;
        Ok(Some(candidates))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SESSION: &str = "01a12ada-edfc-75a3-a451-d6312dd93d6d";
    fn header(fork: bool) -> String {
        let mut value = serde_json::json!({"type":"session_meta","timestamp":"2026-11-04T00:00:10Z","payload":{"id":SESSION}});
        if fork {
            value["payload"]["forked_from_id"] = "parent".into();
        }
        format!("{value}\n")
    }
    fn turn(id: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type":"turn_context","payload":{"turn_id":id}})
        )
    }
    fn quota(at: &str, used: u8) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count","rate_limits":{"primary":{"window_minutes":300,"used_percent":used}}}})
        )
    }
    fn tail(path: &Path) -> Tail {
        Tail {
            source: SourceFile {
                source: Source::Codex,
                path: path.into(),
                modified: crate::time::now_epoch_secs() as i64,
            },
            cursor: None,
        }
    }
    #[test]
    fn bootstrap_and_append_are_read_only_and_keep_explicit_turns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            format!(
                "{}{}{}",
                header(false),
                turn("first"),
                quota("2026-11-04T00:00:11Z", 20)
            ),
        )
        .unwrap();
        let mut reader = tail(&path);
        let initial = reader.read().unwrap().unwrap();
        assert_eq!(initial[0].turn, "first");
        assert_eq!(initial[0].windows[0].remaining_percent, 80);
        assert!(reader.read().unwrap().is_none());
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(
            file,
            "{}{}",
            turn("second"),
            quota("2026-11-04T00:00:12Z", 30)
        )
        .unwrap();
        let added = reader.read().unwrap().unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].turn, "second");
        assert_eq!(added[0].windows[0].remaining_percent, 70);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn bounded_tail_without_turn_does_not_borrow_a_turn_from_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut text = format!("{}{}", header(false), turn("old"));
        let padding = format!("{{\"padding\":\"{}\"}}\n", "x".repeat(1024));
        while text.len() < BOOTSTRAP_BYTES as usize + READ_BYTES as usize {
            text.push_str(&padding);
        }
        text.push_str(&quota("2026-11-04T00:00:11Z", 20));
        fs::write(&path, &text).unwrap();
        let mut reader = tail(&path);
        assert!(reader.read().unwrap().unwrap().is_empty());
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{}{}", turn("new"), quota("2026-11-04T00:00:12Z", 30)).unwrap();
        assert_eq!(reader.read().unwrap().unwrap()[0].turn, "new");
    }
    #[test]
    fn fork_header_survives_copied_headers_and_filters_parent_quota() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fork.jsonl");
        let parent = serde_json::json!({"type":"session_meta","timestamp":"2026-11-04T00:00:00Z","payload":{"id":"01a12ada-edfc-75a3-a451-d6312dd93d6e"}});
        fs::write(
            &path,
            format!(
                "{}{parent}\n{}{}{}",
                header(true),
                turn("t"),
                quota("2026-11-04T00:00:01Z", 20),
                quota("2026-11-04T00:00:11Z", 30)
            ),
        )
        .unwrap();
        let candidates = tail(&path).read().unwrap().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].session, SESSION);
        assert_eq!(candidates[0].windows[0].remaining_percent, 70);
    }
    #[test]
    fn truncation_and_rotation_reset_the_read_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            format!(
                "{}{}{}",
                header(false),
                turn("old"),
                quota("2026-11-04T00:00:11Z", 20)
            ),
        )
        .unwrap();
        let mut reader = tail(&path);
        assert_eq!(reader.read().unwrap().unwrap().len(), 1);
        fs::write(
            &path,
            format!("{}{}", header(false), quota("2026-11-04T00:00:12Z", 30)),
        )
        .unwrap();
        assert!(reader.read().unwrap().unwrap().is_empty());
        let replacement = dir.path().join("replacement");
        fs::write(
            &replacement,
            format!(
                "{}{}{}",
                header(false),
                turn("rotated"),
                quota("2026-11-04T00:00:13Z", 40)
            ),
        )
        .unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(reader.read().unwrap().unwrap()[0].turn, "rotated");
    }
    #[test]
    fn oversized_append_cannot_block_later_quota() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, format!("{}{}", header(false), turn("old"))).unwrap();
        let mut reader = tail(&path);
        reader.read().unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(
            file,
            "{}\n{}{}{}",
            "x".repeat(READ_BYTES as usize * 2),
            quota("2026-11-04T00:00:11Z", 10),
            turn("new"),
            quota("2026-11-04T00:00:12Z", 30)
        )
        .unwrap();
        assert!(reader.read().unwrap().unwrap().is_empty());
        assert!(reader.read().unwrap().unwrap().is_empty());
        let candidates = reader.read().unwrap().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].turn, "new");
    }

    #[test]
    fn partial_final_record_waits_for_append_without_repeated_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let event = quota("2026-11-04T00:00:11Z", 20);
        fs::write(
            &path,
            format!("{}{}{}", header(false), turn("t"), event.trim_end()),
        )
        .unwrap();
        let mut reader = tail(&path);
        assert!(reader.read().unwrap().unwrap().is_empty());
        assert!(reader.read().unwrap().is_none());
        writeln!(fs::OpenOptions::new().append(true).open(&path).unwrap()).unwrap();
        assert_eq!(reader.read().unwrap().unwrap().len(), 1);
    }

    #[test]
    fn normal_line_at_batch_boundary_is_retried_without_losing_turn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, format!("{}{}", header(false), turn("t"))).unwrap();
        let mut reader = tail(&path);
        reader.read().unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let filler = format!(
            "{{\"padding\":\"{}\"}}\n",
            " ".repeat(READ_BYTES as usize - 100)
        );
        write!(file, "{filler}{}", quota("2026-11-04T00:00:11Z", 20)).unwrap();
        assert!(reader.read().unwrap().unwrap().is_empty());
        let candidates = reader.read().unwrap().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].turn, "t");
    }

    #[test]
    fn incomplete_turn_context_revokes_quota_attribution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            format!(
                "{}{}{{\"type\":\"turn_context\",\"payload\":{{}}}}\n{}",
                header(false),
                turn("old"),
                quota("2026-11-04T00:00:11Z", 20)
            ),
        )
        .unwrap();
        assert!(tail(&path).read().unwrap().unwrap().is_empty());
    }

    #[test]
    fn urgent_work_and_inventory_cannot_displace_ordinary_backlog() {
        let dir = tempfile::tempdir().unwrap();
        let db = store::Store::open_writer(&dir.path().join("history.sqlite3")).unwrap();
        let paths: Vec<_> = (0..4)
            .map(|i| dir.path().join(format!("{i}.jsonl")))
            .collect();
        for path in &paths {
            fs::write(path, header(false)).unwrap();
        }
        let mut ordinary: VecDeque<_> = paths.iter().map(|path| tail(path).source).collect();
        let mut urgent = VecDeque::new();
        enqueue_hot(&mut ordinary, &mut urgent, vec![tail(&paths[3]).source]);
        enqueue_hot(&mut ordinary, &mut urgent, vec![tail(&paths[3]).source]);
        assert_eq!(urgent.len(), 1);
        assert_eq!(ordinary.len(), 3);
        // Even an exhausted time budget permits one batch in each independent queue.
        scan_queue(&db, &mut urgent, Duration::ZERO).unwrap();
        scan_queue(&db, &mut ordinary, Duration::ZERO).unwrap();
        assert!(db.checkpoint(&paths[3]).unwrap().is_some());
        assert!(db.checkpoint(&paths[0]).unwrap().is_some());
        merge_inventory(&mut ordinary, vec![tail(&paths[0]).source]);
        assert_eq!(ordinary.front().unwrap().path, paths[1]);
        assert_eq!(ordinary.back().unwrap().path, paths[0]);
    }

    fn corrupted_turn_requires_a_new_explicit_turn(bad_line: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut bytes = format!("{}{}", header(true), turn("a")).into_bytes();
        bytes.extend_from_slice(bad_line);
        bytes.extend_from_slice(quota("2026-11-04T00:00:11Z", 10).as_bytes());
        bytes.extend_from_slice(turn("b").as_bytes());
        // Valid unrelated records must preserve the explicit turn.
        bytes.extend_from_slice(
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\"}}\n",
        );
        // The original fork boundary must also survive the damaged record.
        bytes.extend_from_slice(quota("2026-11-04T00:00:01Z", 20).as_bytes());
        bytes.extend_from_slice(quota("2026-11-04T00:00:12Z", 30).as_bytes());
        fs::write(&path, bytes).unwrap();
        let candidates = tail(&path).read().unwrap().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].session, SESSION);
        assert_eq!(candidates[0].turn, "b");
        assert_eq!(candidates[0].windows[0].remaining_percent, 70);
    }

    #[test]
    fn damaged_json_revokes_turn_until_an_explicit_replacement() {
        corrupted_turn_requires_a_new_explicit_turn(b"{\"type\":\"turn_context\",broken}\n");
    }

    #[test]
    fn invalid_utf8_revokes_turn_until_an_explicit_replacement() {
        corrupted_turn_requires_a_new_explicit_turn(b"\xff\n");
    }

    fn long_turn_file(path: &Path, size: usize) {
        let mut text = format!("{}{}", header(false), turn("long"));
        let padding = format!("{{\"padding\":\"{}\"}}\n", "x".repeat(1024));
        while text.len() < size {
            text.push_str(&padding);
        }
        text.push_str(&quota("2026-11-04T00:00:11Z", 20));
        fs::write(path, text).unwrap();
    }

    #[test]
    fn bootstrap_recovers_long_turn_before_the_incremental_tail_window() {
        let dir = tempfile::tempdir().unwrap();
        for distance in [1060 * 1024, 2200 * 1024] {
            let path = dir.path().join(format!("{distance}.jsonl"));
            long_turn_file(&path, distance);
            let mut reader = tail(&path);
            let candidates = reader.read().unwrap().unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].turn, "long");
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            write!(file, "{}", quota("2026-11-04T00:00:12Z", 30)).unwrap();
            assert_eq!(
                reader.read().unwrap().unwrap()[0].windows[0].remaining_percent,
                70
            );
        }
    }

    #[test]
    fn bootstrap_budget_defers_large_files_without_starving_them() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.jsonl");
        let second = dir.path().join("second.jsonl");
        long_turn_file(&first, 3 * 1024 * 1024);
        long_turn_file(&second, 3 * 1024 * 1024);
        let mut hot = HotFiles {
            files: vec![tail(&first), tail(&second)],
        };
        let mut candidates = QuotaCandidates::default();
        let changed = hot.poll(&mut candidates);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].path, first);
        let changed = hot.poll(&mut candidates);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].path, second);
        assert!(hot.poll(&mut candidates).is_empty());
    }

    #[test]
    fn invalid_large_header_cannot_starve_another_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("broken.jsonl");
        let good = dir.path().join("good.jsonl");
        fs::write(&broken, "x".repeat(3 * 1024 * 1024)).unwrap();
        long_turn_file(&good, 3 * 1024 * 1024);
        let mut hot = HotFiles {
            files: vec![tail(&broken), tail(&good)],
        };
        let mut candidates = QuotaCandidates::default();
        assert!(hot.poll(&mut candidates).is_empty());
        let changed = hot.poll(&mut candidates);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].path, good);
    }

    #[test]
    fn followers_tail_without_changing_writer_checkpoints() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite3");
        let writer = store::Store::open_writer(&db_path).unwrap();
        let follower = store::Store::open_reader(&db_path).unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            format!(
                "{}{}{}",
                header(false),
                turn("t"),
                quota("2026-11-04T00:00:11Z", 20)
            ),
        )
        .unwrap();
        let version = follower.data_version().unwrap();
        assert_eq!(tail(&path).read().unwrap().unwrap().len(), 1);
        assert!(writer.checkpoint(&path).unwrap().is_none());
        writer.set_meta("test", "commit").unwrap();
        assert_ne!(follower.data_version().unwrap(), version);
    }
    #[test]
    fn hot_discovery_is_bounded_and_keeps_idle_files() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<_> = (0..MAX_FILES + 4)
            .map(|i| tail(&dir.path().join(format!("{i}.jsonl"))).source)
            .collect();
        let mut hot = HotFiles::default();
        let now = crate::time::now_epoch_secs();
        hot.discover(&files, now);
        assert_eq!(hot.files.len(), MAX_FILES);
        hot.discover(&files, now + RECENT + 1);
        assert_eq!(hot.files.len(), MAX_FILES);
    }
}
