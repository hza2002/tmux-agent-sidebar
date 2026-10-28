//! Durable metering storage. Every cursor advances in the same transaction as
//! its evidence and effective usage; source disappearance never deletes history.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::evidence::{self, Evidence};
use crate::usage::{Family, Record, RecordId, Tokens, metering};

const APPLICATION_ID: i64 = 0x5441_5355;
const SCHEMA_VERSION: i64 = 1;
const NORMALIZER_VERSION: &str = "1";

/// Aggregation reads countable fields only. The ledger has already resolved
/// identity; decoding its large cumulative identity again creates avoidable
/// allocation work. Full identities remain in stored records and exports.
#[derive(Deserialize)]
struct CountedUsage {
    codex: Option<metering::EventMeta>,
    ts: i64,
    family: Option<Family>,
    tokens: Tokens,
}
const MIN_SQLITE_VERSION: i32 = 3_051_003;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FileCheckpoint {
    pub stamp: (u64, u64, i64, i64, u64),
    pub generation: u64,
    pub offset: u64,
    pub context: String,
    pub prefix_len: usize,
    pub prefix_hash: u64,
    pub boundary_hash: u64,
    pub extractor_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Source, history::evidence::Context};

    #[test]
    fn runtime_attribution_overrides_creator_without_rewriting_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let db = Store::open_writer(&temp.path().join("history.sqlite3")).unwrap();
        let session = "01a12ada-edfc-75a3-a451-d6312dd93d6d";
        let source = Path::new("/synthetic/runtime.jsonl");
        let mut context = Context::default();
        let mut entries = Vec::new();
        evidence::parse_line(
            Source::Codex,
            &json!({"type":"session_meta","payload":{"id":session,"creator_account_id":"creator"}})
                .to_string(),
            source,
            0,
            &mut context,
        )
        .unwrap();
        for (i, turn) in ["historical", "a", "b", "embedded", "missing", "conflict"]
            .iter()
            .enumerate()
        {
            evidence::parse_line(
                Source::Codex,
                &json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol","turn_id":turn}})
                    .to_string(),
                source,
                0,
                &mut context,
            )
            .unwrap();
            entries.extend(evidence::parse_line(Source::Codex,&json!({"type":"event_msg","timestamp":format!("2026-10-28T00:00:0{i}Z"),"payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10},"total_token_usage":{"input_tokens":100*(i+1),"output_tokens":10*(i+1)}}}}).to_string(),source,i as u64+1,&mut context).unwrap());
        }
        db.apply_batch(source, None, &entries, &checkpoint(100), &[])
            .unwrap();
        let start = crate::usage::parse_timestamp("2026-10-28T00:00:00Z").unwrap();
        db.set_meta("codex_attribution_since", &(start + 1).to_string())
            .unwrap();
        for (turn, runtime, account) in [
            ("a", "daemon", Some("pro")),
            ("b", "daemon", Some("plus")),
            ("embedded", "embedded", None),
            ("conflict", "daemon", Some("pro")),
            ("conflict", "daemon", Some("plus")),
        ] {
            db.store_attribution(&crate::usage::attribution::Observation {
                session: session.into(),
                turn: turn.into(),
                observed_at: (start + 1) as u64,
                runtime: runtime.into(),
                account: account.map(str::to_owned),
            })
            .unwrap();
        }
        let mut accounts = Vec::new();
        db.visit_records(start, start + 10, |r| {
            accounts.push((r.ts, r.codex.unwrap().account))
        })
        .unwrap();
        accounts.sort();
        assert_eq!(
            accounts
                .iter()
                .map(|(_, a)| a.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("creator"), Some("pro"), Some("plus"), None, None, None]
        );
        assert!(
            db.records(start, start + 10).unwrap().iter().all(|r| r
                .codex
                .as_ref()
                .unwrap()
                .account
                .as_deref()
                == Some("creator"))
        );
        // A delayed observation corrects a previously unknown turn; duplicate
        // imports do not change counts. Conflicts remain unknown after reopening.
        let late = crate::usage::attribution::Observation {
            session: session.into(),
            turn: "missing".into(),
            observed_at: (start + 4) as u64,
            runtime: "daemon".into(),
            account: Some("plus".into()),
        };
        let spool = crate::usage::attribution::spool(&db.path);
        fs::create_dir(&spool).unwrap();
        // Interrupted publishers must not consume the ready-file batch limit.
        for i in 0..300 {
            fs::write(spool.join(format!("orphan-{i}.pending")), b"").unwrap();
        }
        let queued = spool.join("late.json");
        fs::write(&queued, serde_json::to_vec(&late).unwrap()).unwrap();
        db.import_attribution((start + 10) as u64).unwrap();
        assert!(!queued.exists());
        db.store_attribution(&late).unwrap();
        let status = db.status().unwrap();
        assert_eq!(status["accounts"]["plus"], 2);
        assert_eq!(status["unattributed_events"], 2);
        let backup = temp.path().join("backup.sqlite3");
        db.backup(&backup).unwrap();
        assert_eq!(
            Store::open_reader(&backup)
                .unwrap()
                .codex_account_counts()
                .unwrap(),
            db.codex_account_counts().unwrap()
        );
        let export = temp.path().join("export.jsonl");
        db.export(&export).unwrap();
        assert!(
            fs::read_to_string(export)
                .unwrap()
                .contains("runtime_attribution")
        );
    }

    fn checkpoint(offset: u64) -> FileCheckpoint {
        FileCheckpoint {
            stamp: (1, 2, 3, 4, offset),
            generation: 1,
            offset,
            context: "{}".into(),
            prefix_len: 0,
            prefix_hash: 0,
            boundary_hash: 0,
            extractor_version: 1,
        }
    }

    fn item(output: u64) -> Evidence {
        evidence::parse_line(Source::Claude, &json!({
            "timestamp":"2026-10-26T01:02:03.123Z", "sessionId":"session", "requestId":"request",
            "cwd":"/example/project", "message":{"id":"message", "model":"deepseek-flash",
            "usage":{"input_tokens":100,"cache_read_input_tokens":20,"output_tokens":output}}
        }).to_string(), Path::new("/example/source.jsonl"), 0, &mut Context::default()).unwrap().remove(0)
    }

    fn count(store: &Store, table: &str) -> i64 {
        store
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_unmatched_response_is_pending_not_history_failure() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let path = Path::new("/source");
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"s"}}),
            json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
        ] {
            evidence::parse_line(Source::Codex, &value.to_string(), path, 0, &mut context).unwrap();
        }
        let response = json!({"type":"token_usage_record","timestamp":"2026-10-26T01:00:00.123Z","payload":{"response_id":"r","usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}});
        let entries =
            evidence::parse_line(Source::Codex, &response.to_string(), path, 10, &mut context)
                .unwrap();
        store
            .apply_batch(path, None, &entries, &checkpoint(100), &[])
            .unwrap();
        store.set_meta("inventory_complete", "1").unwrap();
        let health =
            || super::super::read_health(&store, "[]", true, &Ok(temp.path().join("log"))).unwrap();
        assert_eq!(store.pending_response_count().unwrap(), 1);
        assert!(health().notice.is_none(), "fresh pairing is not corruption");
        store
            .connection
            .execute("UPDATE observations SET observed_at=0", [])
            .unwrap();
        assert!(
            health().notice.is_some(),
            "persistent missing evidence stays visible"
        );
        // Fresh traffic and reopening a reader cannot reset the older record's age.
        let reader = Store::open_reader(&temp.path().join("usage.sqlite3")).unwrap();
        assert!(
            super::super::read_health(&reader, "[]", true, &Ok(temp.path().join("log")))
                .unwrap()
                .notice
                .is_some()
        );
        drop(reader);
        let fresh = json!({"type":"token_usage_record","timestamp":"2026-10-26T01:00:01Z","payload":{"response_id":"fresh","usage":{"input_tokens":200,"output_tokens":20,"total_tokens":220}}});
        let entries =
            evidence::parse_line(Source::Codex, &fresh.to_string(), path, 15, &mut context)
                .unwrap();
        store
            .apply_batch(path, Some((1, 100)), &entries, &checkpoint(150), &[])
            .unwrap();
        assert_eq!(store.pending_response_count().unwrap(), 2);
        assert_eq!(store.pending_response_count_before(0).unwrap(), 1);
        assert!(health().notice.is_some());
        let marker = json!({"type":"compacted","timestamp":"2026-10-26T01:00:00.124Z","payload":{"compaction_response_id":"r"}});
        let entries =
            evidence::parse_line(Source::Codex, &marker.to_string(), path, 20, &mut context)
                .unwrap();
        store
            .apply_batch(path, Some((1, 150)), &entries, &checkpoint(200), &[])
            .unwrap();
        assert_eq!(store.pending_response_count().unwrap(), 1);
        assert!(health().notice.is_none());
    }

    #[test]
    fn bundled_sqlite_has_wal_reset_fix_and_store_is_private() {
        assert!(rusqlite::version_number() >= MIN_SQLITE_VERSION);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private/usage.sqlite3");
        let store = Store::open_writer(&path).unwrap();
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(store.status().unwrap()["events"], 0);
    }

    #[test]
    fn transaction_deduplicates_replays_and_replaces_streaming_usage() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let source = Path::new("/example/source.jsonl");
        store
            .apply_batch(source, None, &[item(10)], &checkpoint(100), &[])
            .unwrap();
        store
            .apply_batch(
                source,
                Some((1, 100)),
                &[item(10), item(30)],
                &checkpoint(200),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "observations"), 2);
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 30);
        assert_eq!(store.checkpoint(source).unwrap().unwrap().offset, 200);
        assert!(store.get_meta("last_success").unwrap().is_some());
    }

    #[test]
    fn failure_before_checkpoint_rolls_back_evidence_and_effective_record() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        store.connection.execute_batch("CREATE TRIGGER fail_cursor BEFORE INSERT ON source_files BEGIN SELECT RAISE(ABORT,'injected checkpoint failure'); END;").unwrap();
        let source = Path::new("/example/source.jsonl");
        assert!(
            store
                .apply_batch(source, None, &[item(10)], &checkpoint(100), &[])
                .is_err()
        );
        assert_eq!(count(&store, "observations"), 0);
        assert_eq!(count(&store, "events"), 0);
        assert!(store.checkpoint(source).unwrap().is_none());
        store
            .connection
            .execute_batch("DROP TRIGGER fail_cursor;")
            .unwrap();
        store
            .apply_batch(source, None, &[item(10)], &checkpoint(100), &[])
            .unwrap();
        assert_eq!(count(&store, "events"), 1);
    }

    #[test]
    fn stale_cursor_and_read_only_writes_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite3");
        let store = Store::open_writer(&path).unwrap();
        let source = Path::new("/example/source.jsonl");
        store
            .apply_batch(source, None, &[item(10)], &checkpoint(100), &[])
            .unwrap();
        assert!(
            store
                .apply_batch(source, None, &[item(30)], &checkpoint(200), &[])
                .is_err()
        );
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 10);
        let mut reader = Store::open_reader(&path).unwrap();
        assert!(
            reader
                .apply_batch(source, Some((1, 100)), &[], &checkpoint(200), &[])
                .is_err()
        );
        assert!(reader.set_meta("unauthorized", "value").is_err());
        assert!(reader.purge().is_err());
    }

    #[test]
    fn one_collector_readers_share_commits_and_lock_releases_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite3");
        let owner = Store::open_writer(&path).unwrap();
        assert!(
            Store::open_writer(&path)
                .err()
                .unwrap()
                .starts_with("collector busy")
        );
        let reader = Store::open_reader(&path).unwrap();
        assert_eq!(count(&reader, "events"), 0);
        owner
            .apply_batch(
                Path::new("/example/source.jsonl"),
                None,
                &[item(10)],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        assert_eq!(count(&reader, "events"), 1);
        drop(owner);
        assert!(Store::open_writer(&path).is_ok());
    }

    #[test]
    fn absent_corrupt_and_future_schema_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite3");
        assert!(Store::open_reader(&path).is_err());
        assert!(!path.exists());
        fs::write(&path, b"not a database").unwrap();
        assert!(Store::open_writer(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not a database");
        fs::remove_file(&path).unwrap();
        let owner = Store::open_writer(&path).unwrap();
        owner
            .connection
            .pragma_update(None, "user_version", 99)
            .unwrap();
        drop(owner);
        assert!(
            Store::open_writer(&path)
                .err()
                .unwrap()
                .contains("unsupported")
        );
        let raw = Connection::open(&path).unwrap();
        assert_eq!(
            raw.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            99
        );
        raw.pragma_update(None, "user_version", SCHEMA_VERSION)
            .unwrap();
        raw.execute(
            "UPDATE metadata SET value='future-normalizer' WHERE key='normalizer_version'",
            [],
        )
        .unwrap();
        drop(raw);
        assert!(
            Store::open_writer(&path)
                .err()
                .unwrap()
                .contains("normalization version")
        );
        assert!(
            Store::open_reader(&path)
                .err()
                .unwrap()
                .contains("normalization version")
        );
        let raw = Connection::open(&path).unwrap();
        assert_eq!(
            raw.query_row(
                "SELECT value FROM metadata WHERE key='normalizer_version'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "future-normalizer"
        );
        let bad_directory = temp.path().join("bad-directory");
        fs::write(&bad_directory, b"unrelated existing file").unwrap();
        assert!(Store::open_writer(&bad_directory.join("usage.sqlite3")).is_err());
        assert_eq!(fs::read(bad_directory).unwrap(), b"unrelated existing file");
    }

    #[test]
    fn verified_backup_survives_purge_and_export_has_only_selected_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite3");
        let mut owner = Store::open_writer(&path).unwrap();
        owner
            .apply_batch(
                Path::new("/example/source.jsonl"),
                None,
                &[item(10)],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        let backup = temp.path().join("manual.sqlite3");
        let reader = Store::open_reader(&path).unwrap();
        reader.backup(&backup).unwrap();
        assert!(reader.backup(&backup).is_err());
        assert_eq!(
            Store::open_reader(&backup)
                .unwrap()
                .records(0, i64::MAX)
                .unwrap()
                .len(),
            1
        );
        let export = temp.path().join("export.jsonl");
        reader.export(&export).unwrap();
        let lines: Vec<Value> = fs::read_to_string(&export)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["version"], 1);
        assert_eq!(lines[1]["type"], "observation");
        assert!(reader.export(&export).is_err());
        let automatic = owner.purge().unwrap();
        assert_eq!(count(&owner, "events"), 0);
        assert_eq!(count(&owner, "source_files"), 0);
        assert_eq!(
            Store::open_reader(&automatic)
                .unwrap()
                .records(0, i64::MAX)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn issue_lifecycle_and_range_bounds_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let issue = StoreIssue {
            category: "source".into(),
            message: "unreadable source".into(),
            path: Some("/example/source.jsonl".into()),
            offset: None,
        };
        store.record_issue(&issue).unwrap();
        store.record_issue(&issue).unwrap();
        assert_eq!(store.issue_count().unwrap(), 1);
        assert_eq!(store.unresolved_issues().unwrap()[0]["occurrences"], 2);
        store
            .resolve_issues("/example/source.jsonl", "source")
            .unwrap();
        assert_eq!(store.issue_count().unwrap(), 0);
        store.record_issue(&issue).unwrap();
        store.clear_issues("/example/source.jsonl").unwrap();
        assert_eq!(store.issue_count().unwrap(), 0);
        let item = item(10);
        let ts = item.record.as_ref().unwrap().ts;
        store
            .apply_batch(
                Path::new("/example/source.jsonl"),
                None,
                &[item],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        assert_eq!(store.records(ts, ts + 1).unwrap().len(), 1);
        assert!(store.records(ts - 1, ts).unwrap().is_empty());
    }

    #[test]
    fn explicit_claude_sidechain_relation_does_not_double_count() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let parent = item(30);
        let mut copy = parent.clone();
        copy.key = "sidechain-request".into();
        copy.request = Some("sidechain-request".into());
        copy.sidechain = Some(true);
        copy.preference -= 1i64 << 60;
        copy.canonical = format!("{}-sidechain", copy.canonical);
        for (index, entry) in [copy, parent].into_iter().enumerate() {
            store
                .apply_batch(
                    Path::new("/source"),
                    (index > 0).then_some((1, index as u64)),
                    &[entry],
                    &checkpoint(index as u64 + 1),
                    &[],
                )
                .unwrap();
        }
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 30);
    }

    #[test]
    fn forked_prefix_is_counted_once_in_both_import_orders() {
        let usage = json!({"input_tokens":100,"cached_input_tokens":20,"output_tokens":10,"total_tokens":110});
        let event = json!({"type":"event_msg","timestamp":"2026-10-26T01:00:00.123Z","payload":{"type":"token_count","info":{"last_token_usage":usage,"total_token_usage":usage}}});
        let parse = |session: &str, parent: Option<&str>| {
            let path = PathBuf::from(format!("/source/{session}.jsonl"));
            let mut context = Context::default();
            for value in [
                json!({"type":"session_meta","timestamp":"2026-10-26T01:00:01Z","payload":{"id":session,"forked_from_id":parent}}),
                json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
            ] {
                evidence::parse_line(Source::Codex, &value.to_string(), &path, 0, &mut context)
                    .unwrap();
            }
            evidence::parse_line(Source::Codex, &event.to_string(), &path, 100, &mut context)
                .unwrap()
                .remove(0)
        };
        let parent = parse("parent", None);
        let child = parse("child", Some("parent"));
        assert_eq!(child.related_key.as_ref(), Some(&parent.key));
        for order in [
            [parent.clone(), child.clone()],
            [child.clone(), parent.clone()],
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
            for (index, entry) in order.into_iter().enumerate() {
                store
                    .apply_batch(
                        Path::new("/source"),
                        (index > 0).then_some((1, index as u64)),
                        &[entry],
                        &checkpoint(index as u64 + 1),
                        &[],
                    )
                    .unwrap();
            }
            assert_eq!(count(&store, "events"), 1);
            assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.total(), 110);
        }
    }

    #[test]
    fn conflict_after_alias_discovery_preserves_one_prior_contribution() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let mut first = item(10);
        first.source = "codex".into();
        first.kind = "token_count".into();
        let mut second = item(30);
        second.source = "codex".into();
        second.kind = "token_count".into();
        second.key = "second".into();
        second.offset = 10;
        store
            .apply_batch(
                Path::new("/source"),
                None,
                &[first.clone(), second.clone()],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 2);
        second.related_key = Some(first.key);
        second.canonical.push_str("-linked");
        store
            .apply_batch(
                Path::new("/source"),
                Some((1, 100)),
                &[second],
                &checkpoint(200),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 10);
        assert_eq!(store.issue_count().unwrap(), 1);
    }

    #[test]
    fn disk_full_rolls_back_batch_and_can_retry_without_losing_history() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let source = Path::new("/source");
        store
            .apply_batch(source, None, &[item(10)], &checkpoint(100), &[])
            .unwrap();
        let pages: i64 = store
            .connection
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap();
        store
            .connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let mut larger = item(30);
        larger.canonical.push_str(&"x".repeat(128 * 1024));
        let error = store
            .apply_batch(
                source,
                Some((1, 100)),
                &[larger.clone()],
                &checkpoint(200),
                &[],
            )
            .unwrap_err();
        assert!(error.contains("full"), "{error}");
        assert_eq!(store.checkpoint(source).unwrap().unwrap().offset, 100);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 10);
        assert_eq!(count(&store, "observations"), 1);
        store
            .connection
            .pragma_update(None, "max_page_count", i32::MAX)
            .unwrap();
        store
            .apply_batch(source, Some((1, 100)), &[larger], &checkpoint(200), &[])
            .unwrap();
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 30);
    }

    #[test]
    fn backup_failure_prevents_purge_and_preserves_existing_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        store
            .apply_batch(
                Path::new("/source"),
                None,
                &[item(10)],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        fs::write(temp.path().join("backups"), "existing unrelated file").unwrap();
        assert!(store.purge().is_err());
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(
            fs::read_to_string(temp.path().join("backups")).unwrap(),
            "existing unrelated file"
        );
        assert!(
            store
                .backup(&temp.path().join("missing/backup.sqlite3"))
                .is_err()
        );
        assert_eq!(count(&store, "events"), 1);
    }

    #[test]
    #[ignore = "invoked only by the process-crash recovery test"]
    fn crash_writer_child() {
        let Some(path) = std::env::var_os("TAS_USAGE_TEST_CRASH_DB") else {
            return;
        };
        let store = Store::open_writer(Path::new(&path)).unwrap();
        store
            .apply_batch(
                Path::new("/source"),
                Some((1, 100)),
                &[item(30)],
                &checkpoint(200),
                &[],
            )
            .unwrap();
        panic!("crash failpoint did not terminate child");
    }

    #[test]
    fn abrupt_process_exit_preserves_committed_event_and_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite3");
        {
            let store = Store::open_writer(&path).unwrap();
            store
                .apply_batch(
                    Path::new("/source"),
                    None,
                    &[item(10)],
                    &checkpoint(100),
                    &[],
                )
                .unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "usage::history::store::tests::crash_writer_child",
            ])
            .env("TAS_USAGE_TEST_CRASH_DB", &path)
            .env("TAS_USAGE_TEST_CRASH_BEFORE_CURSOR", "1")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let store = Store::open_writer(&path).unwrap();
        assert_eq!(
            store
                .checkpoint(Path::new("/source"))
                .unwrap()
                .unwrap()
                .offset,
            100
        );
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 10);
        assert_eq!(count(&store, "observations"), 1);
        store
            .apply_batch(
                Path::new("/source"),
                Some((1, 100)),
                &[item(30)],
                &checkpoint(200),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 30);
    }

    #[test]
    fn expired_maintenance_deadlines_remove_incomplete_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let deadline = std::time::Instant::now() - Duration::from_secs(1);
        let backup = temp.path().join("timeout-backup.sqlite3");
        assert!(
            store
                .backup_until(&backup, deadline)
                .unwrap_err()
                .contains("timed out")
        );
        assert!(!backup.exists());
        let export = temp.path().join("timeout-export.jsonl");
        assert!(
            store
                .export_until(&export, deadline)
                .unwrap_err()
                .contains("timed out")
        );
        assert!(!export.exists());
        assert!(store.status().is_ok());
    }

    #[test]
    fn canonical_dedup_retains_newly_proven_response_association() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let mut context = Context::default();
        let path = Path::new("/source");
        let parse = |value: Value, offset, context: &mut Context| {
            evidence::parse_line(Source::Codex, &value.to_string(), path, offset, context).unwrap()
        };
        let setup = |context: &mut Context| {
            parse(
                json!({"type":"session_meta","payload":{"id":"s"}}),
                0,
                context,
            );
            parse(
                json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
                1,
                context,
            );
        };
        let usage = json!({"input_tokens":100,"cached_input_tokens":20,"output_tokens":10,"total_tokens":110});
        let response = json!({"type":"token_usage_record","timestamp":"2026-10-26T01:00:00.123Z","payload":{"response_id":"r","usage":usage,"thread_token_usage":usage}});
        setup(&mut context);
        let first_response = parse(response.clone(), 10, &mut context).remove(0);
        let marker=parse(json!({"type":"compacted","timestamp":"2026-10-26T01:00:00.124Z","payload":{"compaction_response_id":"r"}}),20,&mut context).remove(0);
        store
            .apply_batch(
                path,
                None,
                &[first_response.clone(), marker],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        assert_eq!(store.pending_response_count().unwrap(), 0);
        let mut late_context = Context::default();
        setup(&mut late_context);
        let token=parse(json!({"type":"event_msg","timestamp":"2026-10-26T01:00:00.123Z","payload":{"type":"token_count","info":{"last_token_usage":usage,"total_token_usage":usage}}}),30,&mut late_context).remove(0);
        store
            .apply_batch(path, Some((1, 100)), &[token], &checkpoint(200), &[])
            .unwrap();
        assert_eq!(count(&store, "events"), 2);
        let revised_response = parse(response, 10, &mut late_context).remove(0);
        assert_eq!(first_response.canonical, revised_response.canonical);
        assert!(first_response.related_key.is_none());
        assert!(revised_response.related_key.is_some());
        store
            .apply_batch(
                path,
                Some((1, 200)),
                &[revised_response],
                &checkpoint(300),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(count(&store, "observations"), 3);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.total(), 110);
        assert!(store.pending_responses().unwrap().is_empty());
    }

    #[test]
    fn canonical_hash_collisions_do_not_merge_distinct_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let first = item(10);
        let second = item(30);
        store
            .apply_batch(
                Path::new("/source"),
                None,
                std::slice::from_ref(&first),
                &checkpoint(100),
                &[],
            )
            .unwrap();
        // Force the lookup accelerator to collide; exact canonical comparison
        // must still preserve both distinct versions.
        store
            .connection
            .execute(
                "UPDATE observations SET canonical_hash=?1",
                [canonical_hash(&second.canonical)],
            )
            .unwrap();
        store
            .apply_batch(
                Path::new("/source"),
                Some((1, 100)),
                std::slice::from_ref(&second),
                &checkpoint(200),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "observations"), 2);
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.output, 30);
        store
            .connection
            .execute(
                "UPDATE observations SET canonical_hash=?1 WHERE canonical=?2",
                params![canonical_hash(&first.canonical), first.canonical],
            )
            .unwrap();
        store
            .apply_batch(
                Path::new("/source"),
                Some((1, 200)),
                &[first, second],
                &checkpoint(300),
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "observations"), 2);
        let duplicate: Option<String> = store
            .connection
            .query_row(
                "SELECT json_extract(evidence,'$.canonical') FROM observations LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(duplicate.is_none());
        assert!(
            store.storage_metrics().unwrap()["canonical_bytes"]
                .as_i64()
                .unwrap()
                > 0
        );
    }
    #[test]
    fn rewritten_counter_identity_keeps_prior_usage_once_across_later_batches() {
        let parse = |total: u64, turn: &str| {
            let mut context = Context::default();
            for value in [
                json!({"type":"session_meta","payload":{"id":"session"}}),
                json!({"type":"turn_context","payload":{"turn_id":turn,"model":"gpt-6.1-sol"}}),
            ] {
                evidence::parse_line(
                    Source::Codex,
                    &value.to_string(),
                    Path::new("/source"),
                    0,
                    &mut context,
                )
                .unwrap();
            }
            evidence::parse_line(Source::Codex,&json!({"type":"event_msg","timestamp":"2026-10-26T01:00:00.123Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10},"total_token_usage":{"input_tokens":total,"output_tokens":10}}}}).to_string(),Path::new("/source"),50,&mut context).unwrap().remove(0)
        };
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        store
            .apply_batch(
                Path::new("/source"),
                None,
                &[parse(100, "turn")],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        // A prior rewrite batch already committed the new generation.
        let mut rewritten = checkpoint(10);
        rewritten.generation = 2;
        store
            .apply_batch(Path::new("/source"), Some((1, 100)), &[], &rewritten, &[])
            .unwrap();
        rewritten.offset = 100;
        store
            .apply_batch(
                Path::new("/source"),
                Some((2, 10)),
                &[parse(200, "turn")],
                &rewritten,
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(store.records(0, i64::MAX).unwrap()[0].tokens.total(), 110);
        assert_eq!(store.issue_count().unwrap(), 1);
        // Different turn IDs at that location are distinct usage.
        rewritten.generation = 3;
        store
            .apply_batch(
                Path::new("/source"),
                Some((2, 100)),
                &[parse(300, "different-turn")],
                &rewritten,
                &[],
            )
            .unwrap();
        assert_eq!(count(&store, "events"), 2);
    }
    #[test]
    fn aggregation_projection_preserves_pricing_fields_without_decoding_identity() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        store
            .apply_batch(
                Path::new("/source/ds"),
                None,
                &[item(10)],
                &checkpoint(100),
                &[],
            )
            .unwrap();
        let mut context = Context::default();
        let source = Path::new("/source/codex");
        for value in [
            json!({"type":"session_meta","payload":{"id":"s","creator_account_id":"account"}}),
            json!({"type":"turn_context","payload":{"turn_id":"large-identity".repeat(8000),"model":"gpt-6.1-sol","service_tier":"fast"}}),
        ] {
            evidence::parse_line(Source::Codex, &value.to_string(), source, 0, &mut context)
                .unwrap();
        }
        let entries=evidence::parse_line(Source::Codex,&json!({"type":"event_msg","timestamp":"2026-10-26T01:02:03.123Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":60,"output_tokens":10},"total_token_usage":{"input_tokens":100,"cached_input_tokens":60,"output_tokens":10}}}}).to_string(),source,100,&mut context).unwrap();
        store
            .apply_batch(source, None, &entries, &checkpoint(200), &[])
            .unwrap();
        let full = store.records(0, i64::MAX).unwrap();
        assert_eq!(full.len(), 2);
        assert!(
            full.iter()
                .any(|r| matches!(&r.id,RecordId::Codex{identity,..} if identity.len()>100_000))
        );
        let mut projected = Vec::new();
        store
            .visit_records(0, i64::MAX, |r| projected.push(r))
            .unwrap();
        for original in &full {
            let read = projected
                .iter()
                .find(|r| r.codex == original.codex && r.family == original.family)
                .unwrap();
            assert_eq!(read.tokens, original.tokens);
            assert_eq!(read.ts, original.ts);
            assert!(matches!(&read.id,RecordId::Codex{identity,..} if identity.is_empty()));
        }
        let now = full[0].ts + 60;
        let summarize = |records: &[Record]| {
            let mut codex = metering::empty_snapshot(now, 0, false);
            let mut ds = crate::usage::Spend::default();
            let mut cost = 0;
            for record in records {
                if record.codex.is_some() {
                    metering::add_record(&mut codex, record);
                } else {
                    cost += crate::usage::add_record(&mut ds, record);
                }
            }
            (codex, ds.tokens, cost)
        };
        assert_eq!(summarize(&full), summarize(&projected));
    }
    #[test]
    #[ignore = "explicit synthetic health-query benchmark; temporary data only"]
    fn health_query_scale_benchmark() {
        let count: usize = std::env::var("TAS_USAGE_BENCH_EVENTS")
            .expect("explicit size required")
            .parse()
            .unwrap();
        assert!(count <= 1_500_000);
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        let mut sample = serde_json::to_value(item(10)).unwrap();
        sample.as_object_mut().unwrap().remove("canonical");
        sample["source"] = json!("codex");
        sample["session"] = json!("synthetic");
        sample["source_path"] = json!("/synthetic");
        let fixture = serde_json::to_string(&sample).unwrap();
        store.connection.execute("WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n<?1) INSERT INTO observations(event_key,canonical,canonical_hash,evidence,source_path,source_offset,observed_at,related_key) SELECT 'event-'||n,'facts-'||n,n,json_set(?2,'$.kind',CASE WHEN n%20=0 THEN 'token_usage_record' ELSE 'token_count' END,'$.parent_session',CASE WHEN n%20=1 THEN 'parent' ELSE NULL END),'/synthetic',n,0,CASE WHEN n%20=1 THEN json_array('codex_count','parent','turn',0,n) ELSE NULL END FROM seq",params![count as i64,fixture]).unwrap();
        store.connection.execute("INSERT INTO events(event_key,ts,record,observation_id,preference) SELECT event_key,0,'{}',id,0 FROM observations WHERE id%20<>0",[]).unwrap();
        let pending_sql = "SELECT COUNT(*) FROM observations o WHERE json_extract(o.evidence,'$.kind')='token_usage_record' AND NOT EXISTS(SELECT 1 FROM events e WHERE e.event_key=o.event_key OR e.event_key=o.related_key)";
        let parent_sql = "SELECT COUNT(*) FROM observations o WHERE json_extract(o.evidence,'$.kind')='token_count' AND json_extract(o.evidence,'$.parent_session') IS NOT NULL AND json_extract(o.related_key,'$[1]')=json_extract(o.evidence,'$.parent_session') AND NOT EXISTS(SELECT 1 FROM observations parent WHERE parent.event_key=o.related_key OR (parent.related_key=o.related_key AND json_extract(parent.evidence,'$.session')=json_extract(o.evidence,'$.parent_session')))";
        let plan = |sql: &str| {
            let mut statement = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        let pending_plan = plan(pending_sql);
        let parent_plan = plan(parent_sql);
        assert!(
            pending_plan
                .iter()
                .any(|p| p.contains("observations_pending_response"))
        );
        assert!(
            parent_plan
                .iter()
                .any(|p| p.contains("observations_parent_link"))
        );
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            assert_eq!(store.pending_response_count().unwrap(), count as u64 / 20);
            assert_eq!(
                store.unresolved_parent_count().unwrap(),
                (count as u64).div_ceil(20)
            );
            times.push(start.elapsed().as_micros());
        }
        times.sort();
        println!(
            "{}",
            json!({"health_fixture_observations":count,"pending_responses":count/20,"unresolved_parents":count.div_ceil(20),"both_health_queries_p50_us":times[10],"both_health_queries_p95_us":times[19],"pending_plan":pending_plan,"parent_plan":parent_plan})
        );
    }
    #[test]
    fn paired_parent_response_resolves_fork_alias_without_child_self_matching() {
        let usage = json!({"input_tokens":100,"cached_input_tokens":20,"output_tokens":10,"total_tokens":110});
        let parse = |session: &str, parent: Option<&str>, response: bool| {
            let path = PathBuf::from(format!("/source/{session}.jsonl"));
            let mut context = Context::default();
            for value in [
                json!({"type":"session_meta","timestamp":"2026-10-26T01:00:01Z","payload":{"id":session,"forked_from_id":parent}}),
                json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
            ] {
                evidence::parse_line(Source::Codex, &value.to_string(), &path, 0, &mut context)
                    .unwrap();
            }
            let mut result = Vec::new();
            if response {
                result.extend(evidence::parse_line(Source::Codex,&json!({"type":"token_usage_record","timestamp":"2026-10-26T01:00:00.123Z","payload":{"response_id":"r","usage":usage,"thread_token_usage":usage}}).to_string(),&path,10,&mut context).unwrap());
            }
            result.extend(evidence::parse_line(Source::Codex,&json!({"type":"event_msg","timestamp":"2026-10-26T01:00:00.123Z","payload":{"type":"token_count","info":{"last_token_usage":usage,"total_token_usage":usage}}}).to_string(),&path,20,&mut context).unwrap());
            (path, result)
        };
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_writer(&temp.path().join("usage.sqlite3")).unwrap();
        for name in ["child-one", "child-two"] {
            let (path, entries) = parse(name, Some("parent"), false);
            store
                .apply_batch(&path, None, &entries, &checkpoint(100), &[])
                .unwrap();
        }
        assert_eq!(store.unresolved_parent_count().unwrap(), 2);
        let (path, parent) = parse("parent", None, true);
        store
            .apply_batch(&path, None, &parent, &checkpoint(100), &[])
            .unwrap();
        assert_eq!(store.unresolved_parent_count().unwrap(), 0);
        assert_eq!(store.pending_response_count().unwrap(), 0);
        assert_eq!(count(&store, "events"), 1);
    }
}

#[derive(Clone, Debug)]
pub(super) struct StoreIssue {
    pub category: String,
    pub message: String,
    pub path: Option<String>,
    pub offset: Option<u64>,
}

struct CollectorLock(File);

impl CollectorLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = private_options()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .map_err(|e| format!("collector lock: {e}"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err(
                    "collector busy: stop active sidebars before destructive maintenance".into(),
                );
            }
            return Err(format!("collector lock: {error}"));
        }
        Ok(Self(file))
    }
}

impl Drop for CollectorLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub(super) struct Store {
    connection: Connection,
    path: PathBuf,
    _lock: Option<CollectorLock>,
}

/// Stable FNV-1a is a lookup accelerator only. Equality always compares the
/// complete canonical evidence, so collisions cannot merge distinct usage.
fn canonical_hash(value: &str) -> i64 {
    value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    }) as i64
}

fn same_source_locator(prior: &Value, incoming: &Evidence) -> bool {
    prior.get("source_path").and_then(Value::as_str) == Some(incoming.source_path.as_str())
        && prior.get("source").and_then(Value::as_str) == Some(incoming.source.as_str())
        && prior.get("kind").and_then(Value::as_str) == Some(incoming.kind.as_str())
        && prior.get("raw_ts").and_then(Value::as_str) == Some(incoming.raw_ts.as_str())
        && [
            ("session", incoming.session.as_deref()),
            ("turn", incoming.turn.as_deref()),
            ("message", incoming.message.as_deref()),
            ("request", incoming.request.as_deref()),
            ("response", incoming.response.as_deref()),
        ]
        .into_iter()
        .all(|(field, value)| prior.get(field).and_then(Value::as_str) == value)
}

fn sql(error: rusqlite::Error) -> String {
    format!("usage database: {error}")
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options
}

fn private_directory(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("usage directory must be a real directory".into());
        }
    } else {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| format!("create usage directory: {e}"))?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("protect usage directory: {e}"))
}

fn validate(connection: &Connection) -> Result<()> {
    let identity: i64 = connection
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(sql)?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql)?;
    if identity != APPLICATION_ID {
        return Err("unrecognized usage database; original file preserved".into());
    }
    if version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported usage database schema {version}; expected {SCHEMA_VERSION}; original file preserved"
        ));
    }
    let normalizer: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='normalizer_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    if normalizer.as_deref() != Some(NORMALIZER_VERSION) {
        return Err("unsupported usage normalization version; backed-up migration required".into());
    }
    Ok(())
}

impl Store {
    pub(super) fn open_writer(path: &Path) -> Result<Self> {
        if rusqlite::version_number() < MIN_SQLITE_VERSION {
            return Err("SQLite 3.51.3 or newer is required".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("usage database needs a parent directory")?;
        private_directory(parent)?;
        let lock = CollectorLock::acquire(&path.with_extension("lock"))?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("usage database must be a regular file".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::create_database(path)?
            }
            Err(error) => return Err(format!("inspect usage database: {error}")),
        }
        let connection =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(sql)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(sql)?;
        validate(&connection)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(sql)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(sql)?;
        let mode: String = connection
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(sql)?;
        let synchronous: i64 = connection
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(sql)?;
        let foreign_keys: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .map_err(sql)?;
        if mode != "wal" || synchronous != 2 || foreign_keys != 1 {
            return Err("usage database durability settings were not applied".into());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            _lock: Some(lock),
        })
    }

    pub(super) fn open_reader(path: &Path) -> Result<Self> {
        let connection =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sql)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(sql)?;
        validate(&connection)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            _lock: None,
        })
    }

    /// Publish only a complete initial schema. A killed first launch may leave
    /// an unused private staging file, never an empty authoritative database.
    fn create_database(path: &Path) -> Result<()> {
        let temporary = path.with_extension(format!(
            "creating-{}-{}",
            std::process::id(),
            crate::time::now_epoch_millis()
        ));
        drop(
            private_options()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| format!("create initial database: {e}"))?,
        );
        let result = (|| {
            let connection =
                Connection::open_with_flags(&temporary, OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(sql)?;
            Self::initialize(&connection)?;
            connection.close().map_err(|(_, error)| sql(error))?;
            File::open(&temporary)
                .and_then(|file| file.sync_all())
                .map_err(|e| format!("sync initial database: {e}"))?;
            // hard_link publishes without replacing an independently created file.
            fs::hard_link(&temporary, path)
                .map_err(|e| format!("publish initial database: {e}"))?;
            if let Some(parent) = path.parent() {
                File::open(parent)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| format!("sync database directory: {e}"))?;
            }
            Ok(())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    fn initialize(connection: &Connection) -> Result<()> {
        connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO metadata(key,value) VALUES('normalizer_version','1');
            CREATE TABLE source_files(path TEXT PRIMARY KEY, generation INTEGER NOT NULL, offset INTEGER NOT NULL, checkpoint TEXT NOT NULL);
            CREATE TABLE observations(id INTEGER PRIMARY KEY, event_key TEXT NOT NULL, canonical TEXT NOT NULL, canonical_hash INTEGER NOT NULL, evidence TEXT NOT NULL, source_path TEXT NOT NULL, source_offset INTEGER NOT NULL, observed_at INTEGER NOT NULL, related_key TEXT);
            CREATE INDEX observations_by_event ON observations(event_key);
            CREATE INDEX observations_by_hash ON observations(canonical_hash);
            CREATE INDEX observations_by_locator ON observations(source_path,source_offset);
            CREATE INDEX observations_by_related ON observations(related_key);
            CREATE INDEX observations_pending_response ON observations(event_key,related_key) WHERE json_extract(evidence,'$.kind')='token_usage_record';
            CREATE INDEX observations_parent_link ON observations(related_key) WHERE json_extract(evidence,'$.kind')='token_count' AND json_extract(evidence,'$.parent_session') IS NOT NULL;
            CREATE INDEX observations_by_message_session ON observations(json_extract(evidence,'$.message'),json_extract(evidence,'$.session')) WHERE json_extract(evidence,'$.message') IS NOT NULL;
            CREATE TABLE events(event_key TEXT PRIMARY KEY, ts INTEGER NOT NULL, record TEXT NOT NULL, observation_id INTEGER NOT NULL REFERENCES observations(id), preference INTEGER NOT NULL);
            CREATE INDEX events_by_time ON events(ts);
            CREATE TABLE issues(issue_key TEXT PRIMARY KEY, category TEXT NOT NULL, message TEXT NOT NULL, path TEXT, offset INTEGER, first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, occurrences INTEGER NOT NULL DEFAULT 1, resolved INTEGER NOT NULL DEFAULT 0);
            PRAGMA application_id=1413567317;
            PRAGMA user_version=1;
            COMMIT;") .map_err(sql)?;
        validate(connection)
    }

    pub(super) fn checkpoint(&self, path: &Path) -> Result<Option<FileCheckpoint>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT checkpoint FROM source_files WHERE path=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        raw.map(|v| serde_json::from_str(&v).map_err(|e| format!("invalid stored checkpoint: {e}")))
            .transpose()
    }

    pub(super) fn apply_batch(
        &self,
        path: &Path,
        expected: Option<(u64, u64)>,
        evidence: &[Evidence],
        checkpoint: &FileCheckpoint,
        issues: &[StoreIssue],
    ) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        let generation =
            i64::try_from(checkpoint.generation).map_err(|_| "generation exceeds SQLite range")?;
        let offset = i64::try_from(checkpoint.offset).map_err(|_| "offset exceeds SQLite range")?;
        let transaction = self.connection.unchecked_transaction().map_err(sql)?;
        let path = path.to_string_lossy();
        let current: Option<(i64, i64)> = transaction
            .query_row(
                "SELECT generation,offset FROM source_files WHERE path=?1",
                [path.as_ref()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let current = current.map(|(g, o)| (g as u64, o as u64));
        if current != expected {
            return Err("usage checkpoint changed; batch was not applied".into());
        }
        let now = crate::time::now_epoch_secs() as i64;
        if current.is_some_and(|(generation, _)| generation != checkpoint.generation) {
            transaction
                .execute(
                    "UPDATE issues SET resolved=1 WHERE path=?1 AND category='source_parse'",
                    [path.as_ref()],
                )
                .map_err(sql)?;
        }
        let mut affected = std::collections::BTreeSet::new();
        let mut revision_keys = std::collections::HashSet::new();
        for item in evidence {
            let mut related_key = item.related_key.clone();
            let hash = canonical_hash(&item.canonical);
            let existing: Option<i64> = transaction
                .query_row(
                    "SELECT id FROM observations WHERE canonical_hash=?1 AND canonical=?2",
                    params![hash, item.canonical],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql)?;
            if let Some(id) = existing {
                if let Some(related_key) = &item.related_key {
                    transaction.execute("UPDATE observations SET related_key=?2 WHERE id=?1 AND related_key IS NULL",params![id,related_key]).map_err(sql)?;
                }
            } else {
                let source_offset =
                    i64::try_from(item.offset).map_err(|_| "source offset exceeds SQLite range")?;
                let mut candidates=transaction.prepare("SELECT event_key,evidence FROM observations WHERE source_path=?1 AND source_offset=?2 AND event_key<>?3 ORDER BY id").map_err(sql)?;
                let rows = candidates
                    .query_map(params![path.as_ref(), source_offset, item.key], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(sql)?;
                for row in rows {
                    let (prior_key, raw) = row.map_err(sql)?;
                    let prior: Value = serde_json::from_str(&raw)
                        .map_err(|e| format!("invalid stored metering evidence: {e}"))?;
                    if same_source_locator(&prior, item) {
                        related_key = Some(prior_key);
                        revision_keys.insert(item.key.clone());
                        Self::issue_on(&transaction,&StoreIssue{category:"source_revision".into(),message:"metering identity changed at the same source location; prior usage retained".into(),path:Some(path.to_string()),offset:Some(item.offset)},now)?;
                        break;
                    }
                }
                let mut encoded = serde_json::to_value(item).map_err(|e| e.to_string())?;
                encoded
                    .as_object_mut()
                    .ok_or("metering evidence is not an object")?
                    .remove("canonical");
                let encoded = serde_json::to_string(&encoded).map_err(|e| e.to_string())?;
                transaction.execute("INSERT INTO observations(event_key,canonical,canonical_hash,evidence,source_path,source_offset,observed_at,related_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![item.key,item.canonical,hash,encoded,path.as_ref(),source_offset,now,related_key]).map_err(sql)?;
            }
            affected.insert(item.key.clone());
            if let Some(key) = &related_key {
                affected.insert(key.clone());
            }
            if let Some(message) = &item.issue {
                Self::issue_on(
                    &transaction,
                    &StoreIssue {
                        category: "metering".into(),
                        message: message.clone(),
                        path: Some(path.to_string()),
                        offset: Some(item.offset),
                    },
                    now,
                )?;
            }
        }
        let mut reconciled = std::collections::HashSet::new();
        for key in affected {
            if reconciled.contains(&key) {
                continue;
            }
            let mut group = Vec::new();
            let mut keys = std::collections::BTreeSet::from([key]);
            let mut loaded = std::collections::HashSet::new();
            loop {
                let pending: Vec<_> = keys
                    .iter()
                    .filter(|key| !loaded.contains(*key))
                    .cloned()
                    .collect();
                if pending.is_empty() {
                    break;
                }
                for key in pending {
                    loaded.insert(key.clone());
                    let mut statement = transaction.prepare("SELECT id,canonical,evidence,related_key FROM observations WHERE event_key=?1 OR related_key=?1 ORDER BY id").map_err(sql)?;
                    let rows = statement
                        .query_map([key], |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                            ))
                        })
                        .map_err(sql)?;
                    for row in rows {
                        let (id, canonical, raw, related_key) = row.map_err(sql)?;
                        if group.iter().any(|(existing, _)| *existing == id) {
                            continue;
                        }
                        let mut value: Value = serde_json::from_str(&raw)
                            .map_err(|e| format!("invalid stored metering evidence: {e}"))?;
                        value["canonical"] = Value::String(canonical);
                        let mut item: Evidence = serde_json::from_value(value)
                            .map_err(|e| format!("invalid stored metering evidence: {e}"))?;
                        item.related_key = related_key;
                        if item.source == "claude"
                            && let (Some(message), Some(session)) = (&item.message, &item.session)
                        {
                            let mut linked = transaction.prepare("SELECT event_key FROM observations WHERE json_extract(evidence,'$.message')=?1 AND json_extract(evidence,'$.session')=?2 AND (?3 OR json_extract(evidence,'$.sidechain')=1)").map_err(sql)?;
                            let linked_keys = linked
                                .query_map(
                                    params![message, session, item.sidechain == Some(true)],
                                    |row| row.get::<_, String>(0),
                                )
                                .map_err(sql)?;
                            for linked in linked_keys {
                                keys.insert(linked.map_err(sql)?);
                            }
                        }
                        keys.insert(item.key.clone());
                        if let Some(key) = &item.related_key {
                            keys.insert(key.clone());
                        }
                        group.push((id, item));
                    }
                }
            }
            reconciled.extend(keys.iter().cloned());
            let items: Vec<_> = group.iter().map(|(_, item)| item.clone()).collect();
            let Some(selection) = evidence::reconcile(&items) else {
                continue;
            };
            if selection.issue.is_some() || keys.iter().any(|key| revision_keys.contains(key)) {
                if let Some(message) = &selection.issue {
                    Self::issue_on(
                        &transaction,
                        &StoreIssue {
                            category: "conflict".into(),
                            message: message.clone(),
                            path: Some(path.to_string()),
                            offset: Some(checkpoint.offset),
                        },
                        now,
                    )?;
                }
                let mut existing = Vec::new();
                for key in &keys {
                    if let Some(observation) = transaction
                        .query_row(
                            "SELECT observation_id FROM events WHERE event_key=?1",
                            [key],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()
                        .map_err(sql)?
                    {
                        existing.push((observation, key.clone()));
                    }
                }
                existing.sort();
                if let Some((_, survivor)) = existing.first() {
                    // New evidence can prove that separately accepted keys are
                    // aliases. Preserve the earliest accepted decision while
                    // ensuring their unresolved conflict is still counted once.
                    for key in &keys {
                        if key != survivor {
                            transaction
                                .execute("DELETE FROM events WHERE event_key=?1", [key])
                                .map_err(sql)?;
                        }
                    }
                    continue;
                }
            }
            if let Some(record) = &selection.record {
                let encoded_record = serde_json::to_string(record).map_err(|e| e.to_string())?;
                let (observation, item) = group
                    .get(selection.selected)
                    .ok_or("invalid evidence selection")?;
                for key in &keys {
                    transaction
                        .execute("DELETE FROM events WHERE event_key=?1", [key])
                        .map_err(sql)?;
                }
                transaction.execute("INSERT INTO events(event_key,ts,record,observation_id,preference) VALUES(?1,?2,?3,?4,?5)",params![selection.key,record.ts,encoded_record,observation,item.preference]).map_err(sql)?;
            }
        }
        for issue in issues {
            Self::issue_on(&transaction, issue, now)?;
        }
        #[cfg(test)]
        if std::env::var_os("TAS_USAGE_TEST_CRASH_BEFORE_CURSOR").is_some() {
            // Test only: force dirty pages into WAL, then kill the process
            // without running Rust destructors or SQLite rollback.
            transaction.cache_flush().map_err(sql)?;
            unsafe { libc::_exit(86) };
        }
        let encoded = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO source_files(path,generation,offset,checkpoint) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET generation=excluded.generation,offset=excluded.offset,checkpoint=excluded.checkpoint", params![path.as_ref(),generation,offset,encoded]).map_err(sql)?;
        transaction.execute("INSERT INTO metadata(key,value) VALUES('last_success',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [now.to_string()]).map_err(sql)?;
        transaction.commit().map_err(sql)
    }

    fn issue_on(connection: &Connection, issue: &StoreIssue, now: i64) -> Result<()> {
        let key =
            serde_json::to_string(&(&issue.category, &issue.path, issue.offset, &issue.message))
                .map_err(|e| e.to_string())?;
        let offset = issue
            .offset
            .map(i64::try_from)
            .transpose()
            .map_err(|_| "issue offset exceeds SQLite range")?;
        connection.execute("INSERT INTO issues(issue_key,category,message,path,offset,first_seen,last_seen) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(issue_key) DO UPDATE SET last_seen=excluded.last_seen,occurrences=issues.occurrences+1,resolved=0", params![key,issue.category,issue.message,issue.path,offset,now]).map_err(sql)?;
        Ok(())
    }

    pub(super) fn record_issue(&self, issue: &StoreIssue) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        Self::issue_on(
            &self.connection,
            issue,
            crate::time::now_epoch_secs() as i64,
        )
    }

    pub(super) fn unresolved_issues(&self) -> Result<Vec<Value>> {
        let mut statement = self.connection.prepare("SELECT category,message,path,offset,first_seen,last_seen,occurrences FROM issues WHERE resolved=0 ORDER BY last_seen DESC LIMIT 100").map_err(sql)?;
        statement.query_map([],|r| Ok(json!({"category":r.get::<_,String>(0)?,"message":r.get::<_,String>(1)?,"path":r.get::<_,Option<String>>(2)?,"offset":r.get::<_,Option<i64>>(3)?,"first_seen":r.get::<_,i64>(4)?,"last_seen":r.get::<_,i64>(5)?,"occurrences":r.get::<_,i64>(6)?}))).map_err(sql)?.collect::<std::result::Result<_,_>>().map_err(sql)
    }

    pub(super) fn issue_count(&self) -> Result<u64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM issues WHERE resolved=0", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count as u64)
            .map_err(sql)
    }

    #[cfg(test)]
    pub(super) fn clear_issues(&self, path: &str) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        self.connection
            .execute("UPDATE issues SET resolved=1 WHERE path=?1", [path])
            .map_err(sql)?;
        Ok(())
    }

    pub(super) fn resolve_issues(&self, path: &str, category: &str) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        self.connection
            .execute(
                "UPDATE issues SET resolved=1 WHERE path=?1 AND category=?2",
                params![path, category],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub(super) fn pending_response_count(&self) -> Result<u64> {
        self.pending_response_count_before(i64::MAX)
    }

    pub(super) fn pending_response_count_before(&self, observed_before: i64) -> Result<u64> {
        self.connection.query_row("SELECT COUNT(*) FROM observations o WHERE observed_at<=?1 AND json_extract(o.evidence,'$.kind')='token_usage_record' AND NOT EXISTS(SELECT 1 FROM events e WHERE e.event_key=o.event_key OR e.event_key=o.related_key)",[observed_before],|row|row.get::<_,i64>(0)).map(|count|count as u64).map_err(sql)
    }

    pub(super) fn unresolved_parent_count(&self) -> Result<u64> {
        self.connection.query_row("SELECT COUNT(*) FROM observations o WHERE json_extract(o.evidence,'$.kind')='token_count' AND json_extract(o.evidence,'$.parent_session') IS NOT NULL AND json_extract(o.related_key,'$[1]')=json_extract(o.evidence,'$.parent_session') AND NOT EXISTS(SELECT 1 FROM observations parent WHERE parent.event_key=o.related_key OR (parent.related_key=o.related_key AND json_extract(parent.evidence,'$.session')=json_extract(o.evidence,'$.parent_session')))",[],|row|row.get::<_,i64>(0)).map(|count|count as u64).map_err(sql)
    }

    pub(super) fn pending_responses(&self) -> Result<Vec<Value>> {
        let mut statement=self.connection.prepare("SELECT source_path,json_extract(evidence,'$.offset'),json_extract(evidence,'$.session'),json_extract(evidence,'$.response') FROM observations o WHERE json_extract(o.evidence,'$.kind')='token_usage_record' AND NOT EXISTS(SELECT 1 FROM events e WHERE e.event_key=o.event_key OR e.event_key=o.related_key) ORDER BY observed_at,id LIMIT 20").map_err(sql)?;
        statement.query_map([],|row|Ok(json!({"path":row.get::<_,String>(0)?,"offset":row.get::<_,Option<i64>>(1)?,"session":row.get::<_,Option<String>>(2)?,"response":row.get::<_,Option<String>>(3)?}))).map_err(sql)?.collect::<std::result::Result<Vec<_>,_>>().map_err(sql)
    }

    pub(super) fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    pub(super) fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        self.connection.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE metadata.value<>excluded.value",params![key,value]).map_err(sql)?;
        Ok(())
    }

    /// Runtime attribution is an additive metadata namespace. Keeping source
    /// evidence unchanged preserves replay/deduplication and schema-v1 readers.
    pub(super) fn import_attribution(&self, now: u64) -> Result<()> {
        if self._lock.is_none() {
            return Err("read-only usage database".into());
        }
        if self.get_meta("codex_attribution_since")?.is_none() {
            self.set_meta("codex_attribution_since", &now.to_string())?;
        }
        let directory = crate::usage::attribution::spool(&self.path);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("read attribution spool: {e}")),
        };
        let ready = entries.filter_map(|entry| match entry {
            Ok(entry) if entry.path().extension().is_some_and(|s| s == "json") => {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error.to_string())),
        });
        for path in ready.take(256) {
            let path = path?;
            let incoming = crate::usage::attribution::read(&path)?;
            self.store_attribution(&incoming)?;
            fs::remove_file(&path).map_err(|e| format!("remove imported observation: {e}"))?;
        }
        Ok(())
    }

    fn store_attribution(&self, incoming: &crate::usage::attribution::Observation) -> Result<()> {
        if !incoming.valid() {
            return Err("invalid runtime observation".into());
        }
        let transaction = self.connection.unchecked_transaction().map_err(sql)?;
        let key = incoming.key();
        let mut observation = match self.get_meta(&key)? {
            Some(raw) => serde_json::from_str::<crate::usage::attribution::Observation>(&raw)
                .map_err(|_| "invalid stored runtime observation")?,
            None => incoming.clone(),
        };
        observation.merge(incoming);
        self.set_meta(
            &key,
            &serde_json::to_string(&observation).map_err(|e| e.to_string())?,
        )?;
        let since = self
            .get_meta("codex_attribution_since")?
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(incoming.observed_at);
        self.set_meta(
            "codex_attribution_since",
            &since.min(incoming.observed_at).to_string(),
        )?;
        transaction.commit().map_err(sql)
    }

    pub(super) fn visit_records(
        &self,
        start: i64,
        end: i64,
        mut visit: impl FnMut(Record),
    ) -> Result<()> {
        let since = self
            .get_meta("codex_attribution_since")?
            .and_then(|s| s.parse::<i64>().ok());
        let transaction = self.connection.unchecked_transaction().map_err(sql)?;
        {
            let mut statement = transaction
                .prepare(
                    "SELECT e.record, a.value FROM events e
                    JOIN observations o ON o.id=e.observation_id
                    LEFT JOIN metadata a ON a.key='codex_turn:' || json_array(
                        json_extract(o.evidence,'$.session'),json_extract(o.evidence,'$.turn'))
                    WHERE e.ts>=?1 AND e.ts<?2",
                )
                .map_err(sql)?;
            let mut rows = statement.query(params![start, end]).map_err(sql)?;
            while let Some(row) = rows.next().map_err(sql)? {
                let value = row.get_ref(0).map_err(sql)?;
                let raw = value
                    .as_str()
                    .map_err(|_| "stored metering record is not UTF-8 text")?;
                let mut usage: CountedUsage = serde_json::from_str(raw)
                    .map_err(|e| format!("invalid stored metering record: {e}"))?;
                if let Some(meta) = usage.codex.as_mut() {
                    let observation: Option<String> = row.get(1).map_err(sql)?;
                    if let Some(raw) = observation {
                        let attribution: crate::usage::attribution::Observation =
                            serde_json::from_str(&raw).map_err(|_| "invalid stored attribution")?;
                        meta.account = attribution.account;
                    } else if since.is_some_and(|since| usage.ts >= since) {
                        meta.account = None;
                    }
                }
                visit(Record {
                    codex: usage.codex,
                    ts: usage.ts,
                    family: usage.family,
                    tokens: usage.tokens,
                    id: RecordId::Codex {
                        identity: Box::default(),
                        ts: 0,
                        uncached: 0,
                        cached: 0,
                        output: 0,
                    },
                });
            }
        }
        transaction.commit().map_err(sql)
    }

    #[cfg(test)]
    pub(super) fn records(&self, start: i64, end: i64) -> Result<Vec<Record>> {
        let mut statement = self
            .connection
            .prepare("SELECT record FROM events WHERE ts>=?1 AND ts<?2")
            .map_err(sql)?;
        let rows = statement
            .query_map(params![start, end], |row| row.get::<_, String>(0))
            .map_err(sql)?;
        rows.map(|row| {
            let raw = row.map_err(sql)?;
            serde_json::from_str(&raw)
                .map_err(|error| format!("invalid stored metering record: {error}"))
        })
        .collect()
    }

    pub(super) fn status(&self) -> Result<Value> {
        let events: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .map_err(sql)?;
        let observations: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0))
            .map_err(sql)?;
        let files: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM source_files", [], |r| r.get(0))
            .map_err(sql)?;
        let storage = self.storage_metrics()?;
        let (accounts, unattributed_events) = self.codex_account_counts()?;
        Ok(
            json!({"schema":SCHEMA_VERSION,"normalizer_version":NORMALIZER_VERSION,"sqlite":rusqlite::version(),"database":self.path,"collector_active":self.collector_active()?,"events":events,"observations":observations,"files":files,"storage":storage,"last_success":self.get_meta("last_success")?,"importing":self.get_meta("importing")?,"roots":self.get_meta("roots")?,"heartbeat":self.get_meta("heartbeat")?,"pending_responses":self.pending_responses()?,"pending_response_count":self.pending_response_count()?,"unresolved_parent_count":self.unresolved_parent_count()?,"unresolved_issues":self.issue_count()?,"issues":self.unresolved_issues()?,"accounts":accounts,"unattributed_events":unattributed_events,"runtime_attribution":self.attribution_status()?}),
        )
    }

    fn attribution_status(&self) -> Result<Value> {
        let mut counts = BTreeMap::<String, i64>::new();
        let mut statement = self.connection.prepare(
            "SELECT json_extract(value,'$.runtime'),COUNT(*) FROM metadata WHERE key LIKE 'codex_turn:%' GROUP BY 1"
        ).map_err(sql)?;
        for row in statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(sql)?
        {
            let (runtime, count) = row.map_err(sql)?;
            counts.insert(runtime, count);
        }
        Ok(
            json!({"since":self.get_meta("codex_attribution_since")?,"turns":counts,
            "basis":"daemon account observations; embedded/conflicting/missing future identity remains unknown"}),
        )
    }

    /// Effective account counts, including runtime overrides and unknowns.
    fn codex_account_counts(&self) -> Result<(BTreeMap<String, i64>, i64)> {
        let mut accounts = BTreeMap::new();
        let mut unattributed = 0;
        self.visit_records(i64::MIN, i64::MAX, |record| {
            if let Some(meta) = record.codex {
                if let Some(account) = meta.account {
                    *accounts.entry(account).or_insert(0) += 1;
                } else {
                    unattributed += 1;
                }
            }
        })?;
        Ok((accounts, unattributed))
    }

    /// Aggregate-only sizing evidence for status and isolated benchmarks. This
    /// intentionally does not run on the periodic health/query path.
    pub(super) fn storage_metrics(&self) -> Result<Value> {
        let (canonical,evidence):(i64,i64)=self.connection.query_row("SELECT COALESCE(SUM(length(CAST(canonical AS BLOB))),0),COALESCE(SUM(length(CAST(evidence AS BLOB))),0) FROM observations",[],|row|Ok((row.get(0)?,row.get(1)?))).map_err(sql)?;
        let records: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(SUM(length(CAST(record AS BLOB))),0) FROM events",
                [],
                |row| row.get(0),
            )
            .map_err(sql)?;
        let pages: i64 = self
            .connection
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(sql)?;
        let page_size: i64 = self
            .connection
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(sql)?;
        let freelist: i64 = self
            .connection
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .map_err(sql)?;
        let mut statement=self.connection.prepare("SELECT category,CASE WHEN category IN ('metering','source_parse','conflict') THEN message ELSE category END,COUNT(*),SUM(occurrences) FROM issues WHERE resolved=0 GROUP BY 1,2 ORDER BY COUNT(*) DESC").map_err(sql)?;
        let issues:Vec<Value>=statement.query_map([],|row|Ok(json!({"category":row.get::<_,String>(0)?,"reason":row.get::<_,String>(1)?,"count":row.get::<_,i64>(2)?,"occurrences":row.get::<_,i64>(3)?}))).map_err(sql)?.collect::<std::result::Result<_,_>>().map_err(sql)?;
        Ok(
            json!({"database_bytes":pages*page_size,"freelist_bytes":freelist*page_size,"canonical_bytes":canonical,"encoded_evidence_bytes":evidence,"effective_record_bytes":records,"issue_summary":issues}),
        )
    }

    fn collector_active(&self) -> Result<bool> {
        let file = match private_options()
            .read(true)
            .open(self.path.with_extension("lock"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("inspect collector lock: {error}")),
        };
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            Ok(false)
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                Ok(true)
            } else {
                Err(format!("inspect collector lock: {error}"))
            }
        }
    }

    pub(super) fn backup(&self, destination: &Path) -> Result<()> {
        self.backup_until(
            destination,
            std::time::Instant::now() + Duration::from_secs(60),
        )
    }

    fn backup_until(&self, destination: &Path, deadline: std::time::Instant) -> Result<()> {
        let file = private_options()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| format!("create new backup: {e}"))?;
        drop(file);
        let result = (|| {
            let mut target =
                Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(sql)?;
            {
                let backup =
                    rusqlite::backup::Backup::new(&self.connection, &mut target).map_err(sql)?;
                loop {
                    if std::time::Instant::now() >= deadline {
                        return Err("backup timed out; incomplete output removed".into());
                    }
                    match backup.step(256).map_err(sql)? {
                        rusqlite::backup::StepResult::Done => break,
                        _ if std::time::Instant::now() >= deadline => {
                            return Err("backup timed out; source preserved".into());
                        }
                        _ => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            }
            validate(&target)?;
            let integrity: String = target
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(sql)?;
            if integrity != "ok" {
                return Err("backup integrity verification failed".into());
            }
            target.close().map_err(|(_, e)| sql(e))?;
            File::open(destination)
                .and_then(|f| f.sync_all())
                .map_err(|e| format!("sync backup: {e}"))?;
            if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
                File::open(parent)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| format!("sync backup directory: {e}"))?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }

    pub(super) fn export(&self, destination: &Path) -> Result<()> {
        self.export_until(
            destination,
            std::time::Instant::now() + Duration::from_secs(60),
        )
    }

    fn export_until(&self, destination: &Path, deadline: std::time::Instant) -> Result<()> {
        let file = private_options()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| format!("create new export: {e}"))?;
        let mut writer = BufWriter::new(file);
        let transaction = self.connection.unchecked_transaction().map_err(sql)?;
        let result = (|| {
            if std::time::Instant::now() >= deadline {
                return Err("export timed out; incomplete output removed".into());
            }
            writeln!(
                writer,
                "{}",
                json!({"format":"tmux-agent-sidebar-usage","version":1,"schema":SCHEMA_VERSION,"normalizer_version":NORMALIZER_VERSION})
            )
            .map_err(|e| e.to_string())?;
            let mut statement = transaction
                .prepare("SELECT id,json_set(evidence,'$.canonical',canonical,'$.related_key',related_key) FROM observations ORDER BY id")
                .map_err(sql)?;
            let mut rows = statement.query([]).map_err(sql)?;
            while let Some(row) = rows.next().map_err(sql)? {
                if std::time::Instant::now() >= deadline {
                    return Err("export timed out; incomplete output removed".into());
                }
                let id: i64 = row.get(0).map_err(sql)?;
                let raw: String = row.get(1).map_err(sql)?;
                let evidence: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                writeln!(
                    writer,
                    "{}",
                    json!({"type":"observation","id":id,"evidence":evidence})
                )
                .map_err(|e| e.to_string())?;
            }
            drop(rows);
            drop(statement);
            let mut statement = transaction
                .prepare("SELECT event_key,record,observation_id FROM events ORDER BY event_key")
                .map_err(sql)?;
            let mut rows = statement.query([]).map_err(sql)?;
            while let Some(row) = rows.next().map_err(sql)? {
                if std::time::Instant::now() >= deadline {
                    return Err("export timed out; incomplete output removed".into());
                }
                let key: String = row.get(0).map_err(sql)?;
                let raw: String = row.get(1).map_err(sql)?;
                let observation: i64 = row.get(2).map_err(sql)?;
                let record: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                writeln!(
                    writer,
                    "{}",
                    json!({"type":"event","key":key,"record":record,"observation":observation})
                )
                .map_err(|e| e.to_string())?;
            }
            let mut attribution = transaction.prepare(
                "SELECT key,value FROM metadata WHERE key='codex_attribution_since' OR key LIKE 'codex_turn:%' ORDER BY key"
            ).map_err(sql)?;
            for row in attribution
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(sql)?
            {
                if std::time::Instant::now() >= deadline {
                    return Err("export timed out; incomplete output removed".into());
                }
                let (key, value) = row.map_err(sql)?;
                writeln!(writer, "{}", json!({"type":"runtime_attribution","key":key,"value":serde_json::from_str::<Value>(&value).map_err(|e| e.to_string())?})).map_err(|e| e.to_string())?;
            }
            writer.flush().map_err(|e| e.to_string())?;
            writer.get_ref().sync_all().map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            drop(transaction);
            drop(writer);
            let _ = fs::remove_file(destination);
            return result;
        }
        transaction.commit().map_err(sql)
    }

    pub(super) fn purge(&mut self) -> Result<PathBuf> {
        if self._lock.is_none() {
            return Err("collector busy: purge requires the maintenance lock".into());
        }
        let backups = self
            .path
            .parent()
            .ok_or("database has no parent")?
            .join("backups");
        private_directory(&backups)?;
        let path = backups.join(format!(
            "usage-before-purge-{}-{}.sqlite3",
            crate::time::now_epoch_millis(),
            std::process::id()
        ));
        self.backup(&path)?;
        let transaction = self.connection.unchecked_transaction().map_err(sql)?;
        transaction.execute_batch("DELETE FROM events;DELETE FROM observations;DELETE FROM source_files;DELETE FROM issues;DELETE FROM metadata WHERE key<>'normalizer_version';").map_err(sql)?;
        transaction.commit().map_err(sql)?;
        Ok(path)
    }
}
