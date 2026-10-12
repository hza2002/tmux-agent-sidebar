//! Shared serialization and server identity for hook writes and stale cleanup.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) fn socket_path(tmux: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(tmux?.split(',').next()?);
    path.is_absolute()
        .then(|| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

pub(crate) fn server_id(tmux: Option<&str>) -> String {
    let socket = socket_path(tmux);
    let identity = socket
        .as_deref()
        .and_then(Path::to_str)
        .unwrap_or("unknown");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) struct PaneHookLock(std::fs::File);

impl PaneHookLock {
    pub(crate) fn acquire(pane: &str, deadline: Option<Instant>) -> Result<Self, ()> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
        let uid = unsafe { libc::geteuid() };
        let lock_dir = std::env::temp_dir().join(format!("tmux-agent-sidebar-{uid}"));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        if let Err(error) = builder.create(&lock_dir)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(());
        }
        let metadata = std::fs::symlink_metadata(&lock_dir).map_err(|_| ())?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(());
        }
        let pane = pane.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let server = server_id(std::env::var("TMUX").ok().as_deref());
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_dir.join(format!("hook-{server}-{pane}.lock")))
            .map_err(|_| ())?;
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(());
        }
        loop {
            let flags = libc::LOCK_EX | if deadline.is_some() { libc::LOCK_NB } else { 0 };
            if unsafe { libc::flock(file.as_raw_fd(), flags) } == 0 {
                return Ok(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock
                && error.kind() != std::io::ErrorKind::Interrupted
            {
                return Err(());
            }
            if let Some(deadline) = deadline {
                let remaining = deadline.checked_duration_since(Instant::now()).ok_or(())?;
                std::thread::sleep(remaining.min(Duration::from_millis(2)));
            }
        }
    }
}

impl Drop for PaneHookLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_identity_ignores_client_suffix_and_separates_servers() {
        assert_eq!(
            server_id(Some("/tmp/a,1,0")),
            server_id(Some("/tmp/a,99,3"))
        );
        assert_ne!(server_id(Some("/tmp/a,1,0")), server_id(Some("/tmp/b,1,0")));
    }

    #[test]
    fn symlink_socket_names_share_identity() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("socket");
        std::fs::write(&target, "").unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        assert_eq!(server_id(target.to_str()), server_id(alias.to_str()));
    }

    #[test]
    fn contention_is_bounded_and_release_allows_next_writer() {
        let pane = format!("%LOCK_TEST_{}", std::process::id());
        let first = PaneHookLock::acquire(&pane, None).unwrap();
        assert!(
            PaneHookLock::acquire(&pane, Some(Instant::now() + Duration::from_millis(10))).is_err()
        );
        drop(first);
        assert!(
            PaneHookLock::acquire(&pane, Some(Instant::now() + Duration::from_millis(10))).is_ok()
        );
    }
}
