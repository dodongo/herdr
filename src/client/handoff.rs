use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) const RECONNECT_ENV: &str = "HERDR_HANDOFF_RECONNECT";
pub(super) const RECONNECT_WINDOW: Duration = Duration::from_secs(120);

#[cfg(any(unix, test))]
pub(super) fn requested(reason: &str) -> bool {
    reason.contains("reconnect after handoff completes")
}

#[cfg(unix)]
pub(super) fn reexec(reason: &str) -> io::Error {
    use std::os::unix::process::CommandExt;

    let executable = match executable(reason) {
        Ok(path) => path,
        Err(error) => return error,
    };
    std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .env(RECONNECT_ENV, "1")
        .exec()
}

#[cfg(unix)]
fn executable(reason: &str) -> io::Result<std::path::PathBuf> {
    let path = reason
        .split("; ")
        .find_map(|part| part.strip_prefix("exe="))
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .map(Ok)
        .unwrap_or_else(std::env::current_exe)?;
    if !path.is_absolute() {
        return Err(io::Error::other(
            "handoff executable must be an absolute path",
        ));
    }
    Ok(path)
}

pub(super) fn connect<T>(
    retry_window: Duration,
    should_quit: &AtomicBool,
    mut attempt: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let deadline = Instant::now() + retry_window;
    loop {
        if should_quit.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "handoff interrupted",
            ));
        }
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if should_quit.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "handoff interrupted",
                    ));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(error);
                }
                std::thread::sleep(remaining.min(Duration::from_millis(250)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn handoff_uses_the_imported_binary_without_shell_parsing() {
        assert_eq!(
            executable("live update; exe=/opt/custom herdr/bin/herdr").unwrap(),
            std::path::PathBuf::from("/opt/custom herdr/bin/herdr")
        );
        assert!(executable("live update; exe=relative/herdr").is_err());
        assert_eq!(
            executable("live update").unwrap(),
            std::env::current_exe().unwrap()
        );
    }

    #[test]
    fn handoff_reason_does_not_match_ordinary_disconnects() {
        assert!(requested(
            "live update in progress; reconnect after handoff completes; exe=/new/herdr"
        ));
        assert!(!requested("detached"));
        assert!(!requested("server stopped"));
    }

    #[test]
    fn handoff_connection_retries_failed_handshakes() {
        let quit = AtomicBool::new(false);
        let mut attempts = 0;
        let result = connect(Duration::from_secs(2), &quit, || {
            attempts += 1;
            if attempts == 1 {
                Err(io::Error::other("old server rejecting handoff connection"))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 2);
    }

    #[test]
    fn ordinary_startup_does_not_retry_failures() {
        let quit = AtomicBool::new(false);
        let mut attempts = 0;
        let result = connect::<()>(Duration::ZERO, &quit, || {
            attempts += 1;
            Err(io::Error::other("unavailable"))
        });
        assert_eq!(result.unwrap_err().to_string(), "unavailable");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn handoff_retry_stops_on_cancellation() {
        let quit = AtomicBool::new(false);
        let mut attempts = 0;
        let result = connect::<()>(Duration::from_millis(1), &quit, || {
            attempts += 1;
            quit.store(true, Ordering::Release);
            Err(io::Error::other("unavailable"))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(attempts, 1);
    }
}
