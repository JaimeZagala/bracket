//! Single instance per data folder. The first Bracket keeps `instance.lock` open without
//! sharing it; a later one fails to open it, hands its files over through the `requests`
//! folder and exits. Windows closes the file when the process ends, even after a crash, so a
//! stale lock is impossible.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use bracket_session::OpenRequest;
use bracket_session::requests;

const LOCK_FILE: &str = "instance.lock";
/// `ERROR_SHARING_VIOLATION`: another process has the file open.
const SHARING_VIOLATION: i32 = 32;
/// How long a second Bracket waits for the running one to take its request.
const HAND_OVER_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Instance {
    /// This is the running Bracket. The lock lasts as long as the value is alive; `None` if the
    /// lock file could not be created (for example, a read-only folder).
    First(Option<File>),
    /// Another Bracket is running with the same data folder.
    Running,
}

pub fn acquire(data_dir: &Path) -> Instance {
    let _ = fs::create_dir_all(data_dir);
    match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(data_dir.join(LOCK_FILE))
    {
        Ok(file) => Instance::First(Some(file)),
        Err(err) if err.raw_os_error() == Some(SHARING_VIOLATION) => Instance::Running,
        Err(_) => Instance::First(None),
    }
}

/// Gives the files to the running Bracket. Returns false if it did not take them in time (it
/// was closing, or is not responding): then this process should open them itself.
pub fn hand_over(requests_dir: &Path, files: &[OpenRequest]) -> io::Result<bool> {
    // The running Bracket has its own working folder.
    let files: Vec<OpenRequest> = files
        .iter()
        .map(|request| OpenRequest {
            path: std::path::absolute(&request.path).unwrap_or_else(|_| request.path.clone()),
            ..request.clone()
        })
        .collect();
    let request = requests::write_request(requests_dir, &files)?;
    let start = Instant::now();
    while start.elapsed() < HAND_OVER_TIMEOUT {
        if !request.exists() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(20));
    }
    // Withdraw it, so it is not opened twice.
    let _ = fs::remove_file(&request);
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_instance_holds_the_lock() {
        let dir = std::env::temp_dir().join(format!("bracket-instance-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = acquire(&dir);
        assert!(matches!(first, Instance::First(Some(_))));
        assert!(matches!(acquire(&dir), Instance::Running));
        drop(first);
        assert!(
            matches!(acquire(&dir), Instance::First(Some(_))),
            "free again once the first one ends"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_request_nobody_takes_is_withdrawn() {
        let dir = std::env::temp_dir().join(format!("bracket-handover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let taken = hand_over(&dir, &[OpenRequest::file("a.txt")]).unwrap();
        assert!(!taken);
        assert_eq!(requests::take_requests(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
