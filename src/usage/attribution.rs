//! Fork-owned runtime observations. Hooks publish small private files; the
//! existing history collector is the only SQLite writer. No credentials or
//! transcript contents are retained, and missing evidence never guesses identity.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub session: String,
    pub turn: String,
    pub observed_at: u64,
    pub runtime: String,
    pub account: Option<String>,
}

impl Observation {
    pub(crate) fn key(&self) -> String {
        format!(
            "codex_turn:{}",
            serde_json::json!([self.session, self.turn])
        )
    }

    pub(crate) fn valid(&self) -> bool {
        crate::adapter::codex::identity::canonical_uuid(&self.session)
            && !self.turn.is_empty()
            && self.turn.len() <= 256
            && !self.turn.chars().any(char::is_control)
            && matches!(self.runtime.as_str(), "daemon" | "embedded" | "conflict")
            && self.account.as_ref().is_none_or(|id| {
                self.runtime == "daemon"
                    && !id.is_empty()
                    && id.len() <= 256
                    && !id.chars().any(char::is_control)
            })
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        self.observed_at = self.observed_at.min(other.observed_at);
        if self.runtime != other.runtime || self.account != other.account {
            self.runtime = "conflict".into();
            self.account = None;
        }
    }
}

pub(crate) fn spool(database: &Path) -> PathBuf {
    database.with_extension("attribution")
}

pub(crate) fn publish(observation: &Observation) -> Result<(), String> {
    publish_at(&spool(&super::history::database_path()?), observation)
}

fn publish_at(directory: &Path, observation: &Observation) -> Result<(), String> {
    if !observation.valid() {
        return Err("invalid runtime observation".into());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })
        .map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("attribution spool must be a real directory".into());
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let name = format!("{}-{}", std::process::id(), crate::time::now_epoch_millis());
    let pending = directory.join(format!("{name}.pending"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, observation).map_err(|e| e.to_string())?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        // hard_link never replaces another publisher's complete observation.
        fs::hard_link(&pending, directory.join(format!("{name}.json")))
            .map_err(|e| e.to_string())?;
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(pending);
    result
}

pub(crate) fn read(path: &Path) -> Result<Observation, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("runtime observation is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("runtime observation exceeds size limit".into());
    }
    let value: Observation =
        serde_json::from_slice(&bytes).map_err(|_| "invalid runtime observation".to_string())?;
    if !value.valid() {
        return Err("invalid runtime observation identity".into());
    }
    Ok(value)
}

pub(crate) fn daemon_account() -> Option<String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))?;
    crate::adapter::codex::control::account_id(
        &home.join("app-server-control/app-server-control.sock"),
        std::time::Instant::now() + std::time::Duration::from_millis(250),
    )
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_are_private_allowlisted_and_conflicts_are_sticky() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("observations");
        let mut value = Observation {
            session: "01a12ada-edfc-75a3-a451-d6312dd93d6d".into(),
            turn: "turn-a".into(),
            observed_at: 10,
            runtime: "daemon".into(),
            account: Some("a".into()),
        };
        publish_at(&directory, &value).unwrap();
        let path = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(read(&path).unwrap(), value);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut other = value.clone();
        other.account = Some("b".into());
        value.merge(&other);
        assert_eq!(value.runtime, "conflict");
        assert_eq!(value.account, None);
        value.merge(&other);
        assert_eq!(value.runtime, "conflict");
        fs::write(&path, r#"{"secret":"must not import"}"#).unwrap();
        assert!(read(&path).is_err());
    }
}
