//! Failure diagnostics must not depend on the history database being writable.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Diagnostics {
    previous: Option<String>,
    previous_details: Vec<String>,
    occurrences: u64,
}
impl Diagnostics {
    pub fn transition(
        &mut self,
        path: &Path,
        notice: Option<&str>,
        details: &[String],
    ) -> Result<(), String> {
        if self.previous.as_deref() == notice
            && (notice.is_none() || self.previous_details == details)
        {
            self.occurrences = self.occurrences.saturating_add(1);
            return Ok(());
        }
        if self.previous.is_none() && notice.is_none() {
            return Ok(());
        }
        let event = serde_json::json!({"time":crate::time::now_epoch_secs(),"category":if notice.is_some(){"usage_history_failure"}else{"usage_history_recovered"},"message":notice,"details":details,"previous_occurrences":self.occurrences,"sqlite":rusqlite::version()});
        append(
            path,
            &serde_json::to_vec(&event).map_err(|e| e.to_string())?,
        )?;
        self.previous = notice.map(str::to_owned);
        self.previous_details = details.to_vec();
        self.occurrences = 1;
        Ok(())
    }
}

fn append(path: &Path, event: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Diagnostic path has no parent")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create diagnostic directory: {}", e.kind()))?;
    if fs::symlink_metadata(parent)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Diagnostic directory must not be a symlink".into());
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let lock_path = parent.join("usage-history-log.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock_path)
        .map_err(|e| format!("Cannot lock diagnostic log: {}", e.kind()))?;
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Diagnostic log lock busy".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Diagnostic log must not be a symlink".into());
    }
    if fs::metadata(path).is_ok_and(|m| m.len() >= 5 * 1024 * 1024) {
        fs::rename(path, path.with_extension("log.1"))
            .map_err(|e| format!("Cannot rotate diagnostic log: {}", e.kind()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("Cannot open diagnostic log: {}", e.kind()))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    file.write_all(event)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|e| format!("Cannot write diagnostic log: {}", e.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deduplicates_failures_and_records_recovery_without_a_database() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/history.log");
        let mut d = Diagnostics::default();
        for _ in 0..5 {
            d.transition(&path, Some("disk full"), &["commit failed".into()])
                .unwrap();
        }
        d.transition(&path, None, &[]).unwrap();
        let lines = fs::read_to_string(path).unwrap();
        let events: Vec<serde_json::Value> = lines
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["previous_occurrences"], 5);
        assert_eq!(events[1]["category"], "usage_history_recovered");
    }
    #[test]
    fn changed_underlying_cause_is_logged_even_with_same_notice() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.log");
        let mut d = Diagnostics::default();
        d.transition(&path, Some("history failed"), &["disk full".into()])
            .unwrap();
        d.transition(&path, Some("history failed"), &["permission denied".into()])
            .unwrap();
        d.transition(&path, Some("history failed"), &["permission denied".into()])
            .unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("permission denied"));
    }
    #[test]
    fn refuses_symlink_destination() {
        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        fs::write(&victim, "unchanged").unwrap();
        let path = temp.path().join("log");
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        assert!(append(&path, b"error").is_err());
        assert_eq!(fs::read_to_string(victim).unwrap(), "unchanged");
    }
}
