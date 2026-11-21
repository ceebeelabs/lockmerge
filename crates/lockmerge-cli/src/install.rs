//! `lockmerge install`: registers the driver in git's configuration.

use std::io::Write;
use std::process::Command;

pub use lockmerge_core::driver_config::{DRIVER, NAME};

/// The oldest git that expands `%S %X %Y`.
pub const OLDEST_GIT: (u64, u64, u64) = (2, 44, 0);

/// Parses the output of `git --version`, for example
/// `git version 2.50.1 (Apple Git-155)`.
pub fn parse_git_version(text: &str) -> Option<(u64, u64, u64)> {
    let v = text.trim().strip_prefix("git version ")?;
    let v = v.split_whitespace().next()?;
    let mut parts = v.split('.').map(|p| {
        let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u64>().ok()
    });
    let major = parts.next()??;
    let minor = parts.next()??;
    // A missing patch level (`2.44`) is 0; an unparsable one is not a version.
    let patch = parts.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

pub fn is_old(version: (u64, u64, u64)) -> bool {
    version < OLDEST_GIT
}

/// Runs `lockmerge install [--global] [--print]`.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut global = false;
    let mut print_only = false;
    for a in args {
        match a.as_str() {
            "--global" => global = true,
            "--print" => print_only = true,
            other => {
                let _ = writeln!(err, "lockmerge install: unknown option {other}");
                let _ = writeln!(err, "usage: lockmerge install [--global] [--print]");
                return 2;
            }
        }
    }
    let scope = if global { "--global" } else { "--local" };

    match Command::new("git").arg("--version").output() {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            if let Some(v) = parse_git_version(&text)
                && is_old(v)
            {
                let _ = writeln!(
                    err,
                    "lockmerge: warning: {} is older than git {}.{}.{}, which is the oldest that passes the labels \
                     lockmerge needs to read Cargo.toml from git. The driver still runs and uses the lockfile-only rules.",
                    text.trim(),
                    OLDEST_GIT.0,
                    OLDEST_GIT.1,
                    OLDEST_GIT.2
                );
            }
        }
        _ => {
            if !print_only {
                let _ = writeln!(err, "lockmerge install: could not run git");
                return 1;
            }
        }
    }

    // The driver key first: a section with a name and no driver makes git die.
    let entries = [
        ("merge.lockmerge.driver", DRIVER),
        ("merge.lockmerge.name", NAME),
    ];
    if print_only {
        for (key, value) in entries {
            let _ = writeln!(out, "git config {scope} {key} \"{value}\"");
        }
        return 0;
    }
    for (key, value) in entries {
        let status = Command::new("git")
            .args(["config", scope, key, value])
            .output();
        match status {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let _ = writeln!(
                    err,
                    "lockmerge install: git config {key} failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                return 1;
            }
            Err(e) => {
                let _ = writeln!(err, "lockmerge install: could not run git: {e}");
                return 1;
            }
        }
    }
    let place = if global {
        "your global git configuration"
    } else {
        "this repository's git configuration"
    };
    let _ = writeln!(out, "Wrote to {place}:");
    for (key, value) in entries.iter().rev() {
        let _ = writeln!(out, "  {key} = {value}");
    }
    let _ = writeln!(
        out,
        "Next, mark the lockfiles in .gitattributes, for example:\n  Cargo.lock merge=lockmerge"
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse() {
        assert_eq!(
            parse_git_version("git version 2.50.1 (Apple Git-155)\n"),
            Some((2, 50, 1))
        );
        assert_eq!(parse_git_version("git version 2.44.0"), Some((2, 44, 0)));
        assert_eq!(
            parse_git_version("git version 2.45.0.rc1"),
            Some((2, 45, 0))
        );
        assert_eq!(parse_git_version("git version 2.44"), Some((2, 44, 0)));
        assert_eq!(parse_git_version("hello"), None);
        assert_eq!(parse_git_version("git version x.y.z"), None);
    }

    #[test]
    fn old_means_before_2_44_0() {
        assert!(is_old((2, 43, 9)));
        assert!(is_old((2, 39, 5)));
        assert!(!is_old((2, 44, 0)));
        assert!(!is_old((3, 0, 0)));
    }
}
