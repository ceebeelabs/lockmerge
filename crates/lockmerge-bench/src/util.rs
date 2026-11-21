//! Small helpers: hashing, temp directories, running `git` with a timeout.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// A directory removed when dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> io::Result<TempDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lockmerge-bench-{label}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(TempDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// What a finished process left behind.
#[derive(Debug)]
pub struct Ran {
    /// `None` when the process was killed by a signal.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

/// Why a process did not finish.
#[derive(Debug)]
pub enum RunError {
    Spawn(String),
    Timeout,
}

/// Runs `cmd` and waits at most `limit`. Output goes through temp files, not
/// pipes, so a grandchild that outlives a timeout cannot block the reader.
pub fn run(mut cmd: Command, limit: Duration) -> Result<Ran, RunError> {
    let tmp = TempDir::new("run").map_err(|e| RunError::Spawn(e.to_string()))?;
    let out_path = tmp.path().join("out");
    let err_path = tmp.path().join("err");
    let out = fs::File::create(&out_path).map_err(|e| RunError::Spawn(e.to_string()))?;
    let err = fs::File::create(&err_path).map_err(|e| RunError::Spawn(e.to_string()))?;
    cmd.stdin(Stdio::null()).stdout(out).stderr(err);
    let mut child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(RunError::Spawn(e.to_string())),
        }
    };
    Ok(Ran {
        code: status.code(),
        stdout: fs::read(&out_path).unwrap_or_default(),
        stderr: fs::read(&err_path).unwrap_or_default(),
    })
}

/// A `git` command in `dir` that ignores the user's and the system's git
/// configuration, so a replay does not depend on the machine.
pub fn git_isolated(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("GIT_AUTHOR_NAME", "bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.invalid")
        .env("GIT_COMMITTER_NAME", "bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+0000");
    cmd
}

/// A `git` command in `dir` with the user's configuration (proxies,
/// credentials), for the owner-side extraction.
pub fn git_plain(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    cmd
}

/// The day `epoch_secs` falls on, as `YYYY-MM-DD` (UTC).
pub fn civil_date(epoch_secs: u64) -> String {
    // Howard Hinnant's days-to-civil algorithm.
    let z = (epoch_secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's date, UTC.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_date(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(1_763_596_800), "2025-11-20");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn timeout_kills_the_process() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let started = Instant::now();
        assert!(matches!(
            run(cmd, Duration::from_millis(100)),
            Err(RunError::Timeout)
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
