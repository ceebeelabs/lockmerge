//! Test driver: `env-probe %A %S %X %Y %P`. Appends one line to the file named
//! by `LOCKMERGE_PROBE_LOG` saying what identification returned, then exits 1
//! so that git leaves the file conflicted and the test can inspect the state.

use std::io::Write;

use lockmerge_env::{DriverArgs, Git, ManifestResult, Manifests, githead_from_env};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [ours_file, base, ours, theirs, path] = &a[..] else {
        eprintln!("usage: env-probe %A %S %X %Y %P");
        std::process::exit(2);
    };
    let bytes = std::fs::read(ours_file).expect("read %A");
    let args = DriverArgs {
        base_label: base,
        ours_label: ours,
        theirs_label: theirs,
        lockfile_path: path,
        ours_lockfile: &bytes,
    };
    let githead = githead_from_env(std::env::vars_os());
    let git = Git::new();
    let line = match git.identify(&args, &githead) {
        Err(why) => format!("unavailable {:?}", why.0),
        Ok(found) => {
            let m = git.manifests(&args, &githead).merged("Cargo.toml");
            let m = match m {
                ManifestResult::Merged(b) => format!("merged {:?}", String::from_utf8_lossy(&b)),
                ManifestResult::Conflict => "conflict".to_string(),
                ManifestResult::Unavailable(w) => format!("unavailable {:?}", w.0),
            };
            format!(
                "identified base={} ours={} theirs={} manifest={m}",
                found.base, found.ours, found.theirs
            )
        }
    };
    let log = std::env::var("LOCKMERGE_PROBE_LOG").expect("LOCKMERGE_PROBE_LOG");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("open log");
    writeln!(f, "{line}").expect("write log");
    std::process::exit(1);
}
