//! `lockmerge merge-driver` (ADR 0003, ADR 0013).
//!
//! Exit 0: the merged bytes are in `%A`, written to a temporary file in the
//! same directory and renamed over it. Exit 1: a refusal; `%A` holds conflict
//! markers and stderr the reason. Exit 255: an internal error or a panic;
//! `%A` is untouched.

use lockmerge_core::{Context, Format, Manifests, Outcome, Refusal, Report, engine};
use std::fs;
use std::io::{ErrorKind, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub const EXIT_MERGED: i32 = 0;
pub const EXIT_REFUSED: i32 = 1;
pub const EXIT_ERROR: i32 = 255;

/// The arguments git passes: `%O %A %B %L %P` and optionally `%S %X %Y`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// `%O`: the base version.
    pub base: PathBuf,
    /// `%A`: ours. The result goes here.
    pub ours: PathBuf,
    /// `%B`: theirs.
    pub theirs: PathBuf,
    /// `%L`: conflict marker size.
    pub marker_size: usize,
    /// `%P`: the repository path of the file.
    pub path: String,
    /// `%S`, `%X`, `%Y`: labels. Without them the defaults `base`, `ours` and
    /// `theirs` are used, and the commits cannot be identified.
    pub base_label: String,
    pub ours_label: String,
    pub theirs_label: String,
}

impl Args {
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let (five, labels) = match args {
            [_, _, _, _, _] => (args, None),
            [_, _, _, _, _, s, x, y] => (&args[..5], Some((s, x, y))),
            _ => {
                return Err(format!(
                    "merge-driver takes %O %A %B %L %P and optionally %S %X %Y, got {} arguments",
                    args.len()
                ));
            }
        };
        let marker_size = five[3]
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| format!("marker size {:?} is not a positive number", five[3]))?;
        let (base_label, ours_label, theirs_label) = match labels {
            Some((s, x, y)) => (s.clone(), x.clone(), y.clone()),
            None => ("base".into(), "ours".into(), "theirs".into()),
        };
        Ok(Args {
            base: five[0].clone().into(),
            ours: five[1].clone().into(),
            theirs: five[2].clone().into(),
            marker_size,
            path: five[4].clone(),
            base_label,
            ours_label,
            theirs_label,
        })
    }
}

/// Builds the manifest source of a run from the arguments and the bytes of `%A`.
pub type MakeManifests = dyn Fn(&Args, &[u8]) -> Box<dyn Manifests>;

/// What the merge step decided, before anything is written.
enum Decision {
    Merged(Vec<u8>),
    Refused(String),
}

/// Runs the driver and returns its exit code. `manifests` builds the manifest
/// source from the bytes of `%A`; it is called only when a format will run.
pub fn run(
    formats: &[&dyn Format],
    args: &Args,
    manifests: &MakeManifests,
    err: &mut dyn Write,
) -> i32 {
    let base = match fs::read(&args.base) {
        Ok(b) => b,
        // A missing base is the same as an empty one: no ancestor, refused.
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => return internal(err, &format!("cannot read %O: {e}")),
    };
    let ours = match fs::read(&args.ours) {
        Ok(b) => b,
        Err(e) => return internal(err, &format!("cannot read %A: {e}")),
    };
    let theirs = match fs::read(&args.theirs) {
        Ok(b) => b,
        Err(e) => return internal(err, &format!("cannot read %B: {e}")),
    };

    // A panic anywhere in the format, the engine or the manifest source is an
    // internal error. Nothing has been written yet, so `%A` is unchanged.
    let decided = catch_unwind(AssertUnwindSafe(|| {
        decide(formats, args, [&base, &ours, &theirs], manifests)
    }));
    let decision = match decided {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => return internal(err, &e),
        Err(panic) => {
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_string());
            return internal(err, &format!("panic: {what}"));
        }
    };

    match decision {
        Decision::Merged(bytes) => match write_atomically(&args.ours, &bytes) {
            Ok(()) => EXIT_MERGED,
            Err(e) => internal(err, &format!("cannot write %A: {e}")),
        },
        Decision::Refused(reason) => {
            let marked = conflict_bytes(args, &ours, &base, &theirs);
            match write_atomically(&args.ours, &marked) {
                Ok(()) => {
                    let _ = write!(err, "{reason}");
                    EXIT_REFUSED
                }
                Err(e) => internal(err, &format!("cannot write %A: {e}")),
            }
        }
    }
}

fn internal(err: &mut dyn Write, what: &str) -> i32 {
    let _ = writeln!(err, "lockmerge: internal error: {what}");
    let _ = writeln!(err, "The file was left as git wrote it.");
    EXIT_ERROR
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn decide(
    formats: &[&dyn Format],
    args: &Args,
    [base, ours, theirs]: [&[u8]; 3],
    manifests: &MakeManifests,
) -> Result<Decision, String> {
    let name = file_name(&args.path);
    let Some(format) = formats.iter().find(|f| f.matches(name)) else {
        return Ok(Decision::Refused(format!(
            "lockmerge: {} was not merged automatically: lockmerge does not support files named {name}.\n\
             Git's conflict markers are in the file; resolve it by hand.\n",
            args.path
        )));
    };
    if base.is_empty() {
        return Ok(Decision::Refused(format!(
            "lockmerge: {} was not merged automatically: there is no common ancestor version to compare against.\n\
             Git's conflict markers are in the file; resolve it by hand.\n",
            args.path
        )));
    }
    let source = manifests(args, ours);
    let cx = Context::new(source.as_ref());
    let inputs = lockmerge_core::Inputs {
        base,
        ours,
        theirs,
        path: &args.path,
    };
    let report = format.merge(&inputs, &cx).map_err(|e| e.to_string())?;
    Ok(match report.outcome {
        Outcome::Clean(clean) => Decision::Merged(clean.into_bytes()),
        Outcome::Refused(_) => Decision::Refused(refusal_text(*format, &args.path, &report)),
    })
}

/// The plain-language text of a refusal, from the format's `CodeDef` table.
fn refusal_text(format: &dyn Format, path: &str, report: &Report) -> String {
    let Outcome::Refused(refusal) = &report.outcome else {
        return String::new();
    };
    let def = format
        .codes()
        .iter()
        .chain(engine::CORE_CODES)
        .find(|d| d.code == refusal.code);
    let mut text = format!(
        "lockmerge: {path} was not merged automatically ({}).\n",
        refusal.code
    );
    match def {
        Some(def) => {
            text.push_str(def.title);
            text.push_str(".\n");
            text.push_str(&fill(def.explain, refusal));
            text.push('\n');
            text.push_str("Git's conflict markers are in the file. To resolve: ");
            text.push_str(def.next_step);
            text.push('\n');
        }
        None => text.push_str("Git's conflict markers are in the file; resolve it by hand.\n"),
    }
    if !report.also_refused.is_empty() {
        text.push_str(&format!(
            "{} other reason(s) also applied; they are not listed.\n",
            report.also_refused.len()
        ));
    }
    text
}

/// Fills `{subjects}` and `{fact}` placeholders of a `CodeDef` template.
fn fill(template: &str, refusal: &Refusal) -> String {
    let mut out = template.replace("{subjects}", &refusal.subjects.join(", "));
    for (name, value) in &refusal.facts {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

static NEXT: AtomicU64 = AtomicU64::new(0);

fn unique(tag: &str) -> String {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(".lockmerge-{tag}-{}-{n}", std::process::id())
}

/// Writes `bytes` to a temporary file next to `target` and renames it over
/// `target`, keeping the permissions of the old file.
pub fn write_atomically(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let tmp = dir.join(unique("tmp"));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if let Ok(meta) = fs::metadata(target) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn one_line(label: &str) -> String {
    label.replace(['\n', '\r', '\0'], " ")
}

/// The content of `%A` after a refusal: `git merge-file -p` output when it
/// reports conflicts, otherwise one whole-file block (ADR 0003).
fn conflict_bytes(args: &Args, ours: &[u8], base: &[u8], theirs: &[u8]) -> Vec<u8> {
    git_merge_file(args, ours, base, theirs).unwrap_or_else(|| whole_file_block(args, ours, theirs))
}

fn git_merge_file(args: &Args, ours: &[u8], base: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir().join(unique("merge"));
    fs::create_dir(&dir).ok()?;
    let result = (|| {
        for (n, b) in [("ours", ours), ("base", base), ("theirs", theirs)] {
            fs::write(dir.join(n), b).ok()?;
        }
        let out = Command::new("git")
            .current_dir(&dir)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .args(["merge-file", "-p", "--marker-size"])
            .arg(args.marker_size.to_string())
            .args(["-L", &one_line(&args.ours_label)])
            .args(["-L", &one_line(&args.base_label)])
            .args(["-L", &one_line(&args.theirs_label)])
            .args(["ours", "base", "theirs"])
            .output()
            .ok()?;
        // 1 to 127 is that many conflicts; 0 is a clean text merge, which
        // cannot be what a refusal leaves behind.
        match out.status.code() {
            Some(1..=127) => Some(out.stdout),
            _ => None,
        }
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn whole_file_block(args: &Args, ours: &[u8], theirs: &[u8]) -> Vec<u8> {
    let n = args.marker_size;
    let mut out = Vec::new();
    let line = |marker: char, label: &str, out: &mut Vec<u8>| {
        out.extend(std::iter::repeat_n(marker as u8, n));
        if !label.is_empty() {
            out.push(b' ');
            out.extend(one_line(label).bytes());
        }
        out.push(b'\n');
    };
    line('<', &args.ours_label, &mut out);
    out.extend_from_slice(ours);
    if !ours.is_empty() && !ours.ends_with(b"\n") {
        out.push(b'\n');
    }
    line('=', "", &mut out);
    out.extend_from_slice(theirs);
    if !theirs.is_empty() && !theirs.ends_with(b"\n") {
        out.push(b'\n');
    }
    line('>', &args.theirs_label, &mut out);
    out
}
