//! Deadline-bounded subprocess reads for observational routing.
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn output(command: &mut Command, deadline: Instant) -> Result<String, String> {
    const MAX_OUTPUT: usize = 4 * 1024 * 1024;
    if Instant::now() >= deadline {
        return Err("command deadline exceeded".into());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    let result =
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            Err("cannot make command output nonblocking".into())
        } else {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            let mut exited = None;
            loop {
                if Instant::now() >= deadline {
                    break Err("command deadline exceeded".into());
                }
                match stdout.read(&mut buffer) {
                    Ok(0) => {
                        if let Some(status) = exited.or_else(|| child.try_wait().ok().flatten()) {
                            break if status.success() {
                                String::from_utf8(bytes).map_err(|e| e.to_string())
                            } else {
                                Err(format!("command exited with {status}"))
                            };
                        }
                    }
                    Ok(count) => {
                        bytes.extend_from_slice(&buffer[..count]);
                        if bytes.len() > MAX_OUTPUT {
                            break Err("command output exceeds bound".into());
                        }
                        continue;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => break Err(error.to_string()),
                }
                match child.try_wait() {
                    Ok(status) => exited = status,
                    Err(error) => break Err(error.to_string()),
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        };
    // Do not wait for inherited pipe writers. Closing stdout releases our side.
    drop(stdout);
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_success_and_rejects_failure() {
        assert_eq!(
            output(
                Command::new("printf").arg("hello"),
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
            "hello"
        );
        assert!(
            output(
                &mut Command::new("false"),
                Instant::now() + Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn kills_and_reaps_timed_out_child() {
        let start = Instant::now();
        assert!(
            output(
                Command::new("sleep").arg("5"),
                start + Duration::from_millis(20)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
