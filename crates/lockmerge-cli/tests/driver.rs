//! The driver's exit paths with fake formats (ADR 0003, ADR 0013): merged,
//! refused with markers, internal error, panic.

use lockmerge_cli::driver::{self, Args, EXIT_ERROR, EXIT_MERGED, EXIT_REFUSED};
use lockmerge_core::{
    CodeDef, Conflict, Context, Draft, Error, Findings, Format, FormatId, Inputs, ManifestResult,
    Manifests, Outcome, Pipeline, ReasonCode, Refusal, Report, Role, RuleDef, Sealed, Verdict,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const TOY: FormatId = FormatId("toy");

struct NoManifests;
impl Manifests for NoManifests {
    fn merged(&self, _: &str) -> ManifestResult {
        ManifestResult::Conflict
    }
}

fn no_manifests(_: &Args, _: &[u8]) -> Box<dyn Manifests> {
    Box::new(NoManifests)
}

/// Files `name` is `key=value` lines. Handles `Toy.lock`. Keys changed on both
/// sides to different values are refused through the engine (`core.UNPROVEN`).
struct Toy;

impl Pipeline for Toy {
    type Model = BTreeMap<String, String>;
    type Key = String;
    type Unit = String;
    fn id(&self) -> FormatId {
        TOY
    }
    fn rules(&self) -> &'static [RuleDef] {
        &[]
    }
    fn codes(&self) -> &'static [CodeDef] {
        &[]
    }
    fn matches(&self, file_name: &str) -> bool {
        file_name == "Toy.lock"
    }
    fn parse(&self, _: Role, bytes: &[u8]) -> Result<Self::Model, Refusal> {
        let text = String::from_utf8_lossy(bytes);
        Ok(text
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect())
    }
    fn envelope(
        &self,
        _: [&Self::Model; 3],
        _: &Context<'_>,
    ) -> Result<Vec<lockmerge_core::Proof>, Refusal> {
        Ok(Vec::new())
    }
    fn units(&self, m: &Self::Model) -> BTreeMap<String, String> {
        m.clone()
    }
    fn decide(&self, _: &Conflict<'_, String, String>, _: &Context<'_>) -> Verdict<String> {
        Verdict::NotApplicable
    }
    fn validate(&self, _: &Draft<String, String>, _: &Context<'_>) -> Findings {
        Findings::default()
    }
    fn serialize(&self, sealed: &Sealed<String, String>, _: [&Self::Model; 3]) -> Vec<u8> {
        let mut out = String::new();
        for (k, v) in sealed.iter() {
            out.push_str(&format!("{k}={v}\n"));
        }
        out.into_bytes()
    }
}

/// A format that does whatever the test asks, behind the object-safe trait.
enum Misbehaving {
    Panic,
    InternalError,
    Refuse(&'static [CodeDef], Refusal),
}

impl Format for Misbehaving {
    fn id(&self) -> FormatId {
        TOY
    }
    fn matches(&self, name: &str) -> bool {
        name == "Toy.lock"
    }
    fn rules(&self) -> &'static [RuleDef] {
        &[]
    }
    fn codes(&self) -> &'static [CodeDef] {
        match self {
            Misbehaving::Refuse(codes, _) => codes,
            _ => &[],
        }
    }
    fn merge(&self, _: &Inputs<'_>, _: &Context<'_>) -> Result<Report, Error> {
        match self {
            Misbehaving::Panic => panic!("the format panicked on purpose"),
            Misbehaving::InternalError => Err(Error::BadRegistry {
                format: TOY,
                detail: "on purpose".into(),
            }),
            Misbehaving::Refuse(_, r) => Ok(Report {
                format: TOY,
                outcome: Outcome::Refused(r.clone()),
                applied: Vec::new(),
                downgrades: Vec::new(),
                also_refused: Vec::new(),
            }),
        }
    }
}

struct Case {
    dir: PathBuf,
    args: Args,
}

impl Case {
    fn new(tag: &str, path: &str, base: &str, ours: &str, theirs: &str) -> Case {
        let dir =
            std::env::temp_dir().join(format!("lockmerge-cli-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("O"), base).unwrap();
        fs::write(dir.join("A"), ours).unwrap();
        fs::write(dir.join("B"), theirs).unwrap();
        let args = Args {
            base: dir.join("O"),
            ours: dir.join("A"),
            theirs: dir.join("B"),
            marker_size: 7,
            path: path.to_string(),
            base_label: "base-label".into(),
            ours_label: "ours-label".into(),
            theirs_label: "theirs-label".into(),
        };
        Case { dir, args }
    }

    fn run(&self, formats: &[&dyn Format]) -> (i32, String) {
        let mut err = Vec::new();
        let code = driver::run(formats, &self.args, &no_manifests, &mut err);
        (code, String::from_utf8(err).unwrap())
    }

    fn ours(&self) -> String {
        fs::read_to_string(&self.args.ours).unwrap()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn leftovers(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn merged_writes_a_and_exits_0() {
    let c = Case::new(
        "merged",
        "sub/Toy.lock",
        "a=1\n",
        "a=1\nb=2\n",
        "a=1\nc=3\n",
    );
    let (code, err) = c.run(&[&Toy]);
    assert_eq!((code, err.as_str()), (EXIT_MERGED, ""));
    assert_eq!(c.ours(), "a=1\nb=2\nc=3\n");
    // The temporary file is gone: only O, A and B remain.
    assert_eq!(leftovers(&c.dir), ["A", "B", "O"]);
}

#[test]
fn merge_is_symmetric_in_ours_and_theirs() {
    let (o, a, b) = ("a=1\nz=9\n", "a=1\nb=2\n", "a=1\nc=3\nz=9\n");
    let one = Case::new("sym1", "Toy.lock", o, a, b);
    let two = Case::new("sym2", "Toy.lock", o, b, a);
    assert_eq!(one.run(&[&Toy]).0, EXIT_MERGED);
    assert_eq!(two.run(&[&Toy]).0, EXIT_MERGED);
    assert_eq!(one.ours(), two.ours());
}

#[test]
fn refusal_exits_1_with_markers_and_the_reason() {
    // Both sides changed `a` differently: no rule combines them.
    let c = Case::new("refuse", "Toy.lock", "a=1\n", "a=2\n", "a=3\n");
    let (code, err) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_REFUSED);
    assert_eq!(
        c.ours(),
        "<<<<<<< ours-label\na=2\n=======\na=3\n>>>>>>> theirs-label\n"
    );
    assert!(
        err.contains("Toy.lock was not merged automatically (core.UNPROVEN)"),
        "{err}"
    );
    assert!(
        err.contains("No rule could prove this change safe"),
        "{err}"
    );
    assert!(err.contains("Both branches changed a in ways"), "{err}");
    assert!(err.contains("Resolve this entry by hand"), "{err}");
}

#[test]
fn a_format_code_is_explained_from_its_table_with_facts() {
    static CODES: &[CodeDef] = &[CodeDef {
        code: ReasonCode::new(TOY, "R3"),
        title: "Both moved it",
        explain: "{subjects} went to {ours} and {theirs}.",
        next_step: "Pick one.",
        fatal: true,
    }];
    let f = Misbehaving::Refuse(
        CODES,
        Refusal::new(
            ReasonCode::new(TOY, "R3"),
            vec!["serde".into()],
            vec![("ours", "1".into()), ("theirs", "2".into())],
        ),
    );
    let c = Case::new("code", "Toy.lock", "a\n", "b\n", "c\n");
    let (code, err) = c.run(&[&f]);
    assert_eq!(code, EXIT_REFUSED);
    assert!(err.contains("(toy.R3)"), "{err}");
    assert!(err.contains("Both moved it."), "{err}");
    assert!(err.contains("serde went to 1 and 2."), "{err}");
    assert!(err.contains("To resolve: Pick one."), "{err}");
}

#[test]
fn markers_come_from_git_merge_file_with_the_marker_size_and_labels() {
    // Separate regions conflict in git's view only where both changed one:
    // here git reports one conflict and keeps the common lines outside it.
    let c = Case::new(
        "gitmarkers",
        "Toy.lock",
        "top\nmid=1\nbottom\n",
        "top\nmid=2\nbottom\n",
        "top\nmid=3\nbottom\n",
    );
    let (code, _) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_REFUSED);
    assert_eq!(
        c.ours(),
        "top\n<<<<<<< ours-label\nmid=2\n=======\nmid=3\n>>>>>>> theirs-label\nbottom\n"
    );
}

#[test]
fn a_whole_file_block_when_git_sees_no_conflict() {
    // Git merges these cleanly as text, but the format refuses (a changed to
    // different values is not the case here; the format is a refuser).
    static CODES: &[CodeDef] = &[];
    let f = Misbehaving::Refuse(
        CODES,
        Refusal::new(ReasonCode::new(TOY, "R9"), Vec::new(), Vec::new()),
    );
    let c = Case::new("whole", "Toy.lock", "a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n");
    let (code, err) = c.run(&[&f]);
    assert_eq!(code, EXIT_REFUSED);
    assert_eq!(
        c.ours(),
        "<<<<<<< ours-label\nA\nb\nc\n=======\na\nb\nC\n>>>>>>> theirs-label\n"
    );
    // A code the format did not describe still refuses and names the code.
    assert!(err.contains("(toy.R9)"), "{err}");
}

#[test]
fn whole_file_block_ends_unterminated_lines() {
    let f = Misbehaving::Refuse(
        &[],
        Refusal::new(ReasonCode::new(TOY, "R9"), Vec::new(), Vec::new()),
    );
    let c = Case::new("noeol", "Toy.lock", "x", "y", "z");
    // git merge-file reports one conflict here, so make it a case it merges:
    fs::write(&c.args.base, "a\nb\nc").unwrap();
    fs::write(&c.args.ours, "A\nb\nc").unwrap();
    fs::write(&c.args.theirs, "a\nb\nC").unwrap();
    let (code, _) = c.run(&[&f]);
    assert_eq!(code, EXIT_REFUSED);
    assert_eq!(
        c.ours(),
        "<<<<<<< ours-label\nA\nb\nc\n=======\na\nb\nC\n>>>>>>> theirs-label\n"
    );
}

#[test]
fn an_unsupported_file_name_refuses_with_markers() {
    let c = Case::new("unsupported", "sub/other.lock", "a\n", "b\n", "c\n");
    let (code, err) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_REFUSED);
    assert!(c.ours().starts_with("<<<<<<<"), "{}", c.ours());
    assert!(
        err.contains("does not support files named other.lock"),
        "{err}"
    );
}

#[test]
fn an_empty_or_missing_base_refuses_with_markers() {
    let c = Case::new("emptybase", "Toy.lock", "", "a=1\n", "a=1\nb=2\n");
    let (code, err) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_REFUSED);
    assert!(c.ours().contains("<<<<<<<"), "{}", c.ours());
    assert!(err.contains("no common ancestor"), "{err}");

    let c = Case::new("nobase", "Toy.lock", "", "a=1\n", "a=1\nb=2\n");
    fs::remove_file(&c.args.base).unwrap();
    let (code, _) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_REFUSED);
    assert!(c.ours().contains("<<<<<<<"));
}

#[test]
fn a_panic_exits_255_and_leaves_a_unchanged() {
    let c = Case::new("panic", "Toy.lock", "a\n", "b\n", "c\n");
    let (code, err) = c.run(&[&Misbehaving::Panic]);
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(c.ours(), "b\n");
    assert!(err.contains("internal error"), "{err}");
    assert_eq!(leftovers(&c.dir), ["A", "B", "O"]);
}

#[test]
fn an_internal_error_exits_255_and_leaves_a_unchanged() {
    let c = Case::new("internal", "Toy.lock", "a\n", "b\n", "c\n");
    let (code, err) = c.run(&[&Misbehaving::InternalError]);
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(c.ours(), "b\n");
    assert!(err.contains("internal error"), "{err}");
}

#[test]
fn an_unreadable_input_exits_255_and_leaves_a_unchanged() {
    let c = Case::new("unreadable", "Toy.lock", "a\n", "b\n", "c\n");
    fs::remove_file(&c.args.theirs).unwrap();
    let (code, _) = c.run(&[&Toy]);
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(c.ours(), "b\n");
}

#[test]
fn arguments_are_parsed_strictly() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let ok = Args::parse(&s(&["O", "A", "B", "7", "p/Cargo.lock", "s", "x", "y"])).unwrap();
    assert_eq!((ok.marker_size, ok.base_label.as_str()), (7, "s"));
    let no_labels = Args::parse(&s(&["O", "A", "B", "7", "Cargo.lock"])).unwrap();
    assert_eq!(no_labels.ours_label, "ours");
    assert!(Args::parse(&s(&["O", "A", "B", "7"])).is_err());
    assert!(Args::parse(&s(&["O", "A", "B", "7", "p", "s"])).is_err());
    assert!(Args::parse(&s(&["O", "A", "B", "zero", "p"])).is_err());
    assert!(Args::parse(&s(&["O", "A", "B", "0", "p"])).is_err());
}
