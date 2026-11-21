//! The `lockmerge` command line (ADR 0003 as amended by ADR 0010, ADR 0013).
//!
//! - [`driver`] is `lockmerge merge-driver %O %A %B %L %P %S %X %Y`: it picks
//!   the format by the file name of `%P`, runs it, and maps the result to the
//!   exit codes git understands (0 merged, 1 refused with conflict markers,
//!   255 internal error with `%A` unchanged).
//! - [`install`] writes the driver into git's configuration.
//! - [`formats`] is the list of formats the binary knows.
//!
//! The release profile must keep `panic = "unwind"`: the driver maps a panic
//! to exit 255 with `catch_unwind`, which does nothing under `abort`.

#![forbid(unsafe_code)]

pub mod driver;
pub mod formats;
pub mod install;

use std::io::Write;

const USAGE: &str = "\
usage: lockmerge install [--global] [--print]
       lockmerge merge-driver %O %A %B %L %P [%S %X %Y]   (run by git)
";

/// Runs the command line `args` (without the program name) and returns the
/// exit code.
pub fn main_with(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("merge-driver") => merge_driver(&args[1..]),
        Some("install") => install::run(&args[1..], &mut std::io::stdout(), &mut std::io::stderr()),
        Some("help" | "--help" | "-h") => {
            let _ = write!(std::io::stdout(), "{USAGE}");
            0
        }
        _ => {
            let _ = write!(std::io::stderr(), "{USAGE}");
            2
        }
    }
}

fn merge_driver(args: &[String]) -> i32 {
    let mut err = std::io::stderr();
    let parsed = match driver::Args::parse(args) {
        Ok(p) => p,
        Err(e) => {
            // Exit above 128: git reports a failed merge and changes nothing.
            let _ = writeln!(err, "lockmerge: internal error: {e}");
            return driver::EXIT_ERROR;
        }
    };
    let formats = formats::all();
    driver::run(&formats, &parsed, &git_manifests, &mut err)
}

/// The manifests of one driver run, read from git objects.
fn git_manifests(args: &driver::Args, ours: &[u8]) -> Box<dyn lockmerge_env::Manifests> {
    let env_args = lockmerge_env::DriverArgs {
        base_label: &args.base_label,
        ours_label: &args.ours_label,
        theirs_label: &args.theirs_label,
        lockfile_path: &args.path,
        ours_lockfile: ours,
    };
    let githead = lockmerge_env::githead_from_env(std::env::vars_os());
    Box::new(lockmerge_env::Git::new().manifests(&env_args, &githead))
}
