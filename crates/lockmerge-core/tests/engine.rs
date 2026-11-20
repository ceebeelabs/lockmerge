//! Engine tests with a toy format, through the public API only, as a format
//! crate would use it.
//!
//! Toy file: a header line `toy 1`, then sorted `key=value` lines. Unit: the
//! value. Behaviors keyed on the data, so one toy covers every rule:
//!
//! - `toy.M1`: both sides changed a key to different integers: take the larger.
//!   Anything else is `NotApplicable`, except values `bad` (refused with
//!   `toy.R3`) and `fatal14` (refused with the non-fatal `toy.R14`).
//! - prune (`toy.P1`): a unit with value `orphan` is removed.
//! - validate: `toy.V1` passes when no value is empty; an empty value is
//!   refused with `toy.EMPTY`. Two units with value `links` are refused with
//!   the fatal `toy.R15`. A unit with value `soft` adds a non-fatal `toy.R14`.
//! - Keys named `bogus-rule`, `bogus-code` and `bogus-prune` misbehave on
//!   purpose to test the engine's checks.

use lockmerge_core::engine::{self, B1, B2, B3, B4, S1, UNPROVEN};
use lockmerge_core::{
    CodeDef, Conflict, Context, Draft, Error, Findings, Format, FormatId, How, Inputs,
    ManifestResult, Manifests, Outcome, Pipeline, Proof, ReasonCode, Refusal, Removal, Report,
    Role, RuleDef, Sealed, Side, Verdict,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

const TOY: FormatId = FormatId("toy");
const fn code(c: &'static str) -> ReasonCode {
    ReasonCode::new(TOY, c)
}

static RULES: &[RuleDef] = &[
    RuleDef {
        id: "toy.E1",
        summary: "headers agree",
        adr: "0000",
    },
    RuleDef {
        id: "toy.M1",
        summary: "take the larger integer",
        adr: "0000",
    },
    RuleDef {
        id: "toy.P1",
        summary: "prune orphans",
        adr: "0000",
    },
    RuleDef {
        id: "toy.V1",
        summary: "no empty value",
        adr: "0000",
    },
];

const fn def(c: &'static str, fatal: bool) -> CodeDef {
    CodeDef {
        code: code(c),
        title: "t",
        explain: "e",
        next_step: "n",
        fatal,
    }
}
static CODES: &[CodeDef] = &[
    def("PARSE", true),
    def("HEADER", true),
    def("R3", true),
    def("R14", false),
    def("R15", true),
    def("EMPTY", true),
];

#[derive(Default)]
struct Toy {
    serialized: AtomicUsize,
}

type Model = (String, BTreeMap<String, String>);

fn refusal(c: &'static str, subject: &str) -> Refusal {
    Refusal::new(code(c), vec![subject.to_string()], Vec::new())
}

impl Pipeline for Toy {
    type Model = Model;
    type Key = String;
    type Unit = String;

    fn id(&self) -> FormatId {
        TOY
    }
    fn rules(&self) -> &'static [RuleDef] {
        RULES
    }
    fn codes(&self) -> &'static [CodeDef] {
        CODES
    }
    fn matches(&self, file_name: &str) -> bool {
        file_name == "toy.lock"
    }

    fn parse(&self, role: Role, bytes: &[u8]) -> Result<Model, Refusal> {
        let who = format!("{role:?}");
        let text = std::str::from_utf8(bytes).map_err(|_| refusal("PARSE", &who))?;
        let mut lines = text.lines();
        let header = lines
            .next()
            .ok_or_else(|| refusal("PARSE", &who))?
            .to_string();
        let mut map = BTreeMap::new();
        for line in lines {
            let (k, v) = line.split_once('=').ok_or_else(|| refusal("PARSE", &who))?;
            if map.insert(k.to_string(), v.to_string()).is_some() {
                return Err(refusal("PARSE", &who));
            }
        }
        Ok((header, map))
    }

    fn envelope(&self, m: [&Model; 3], cx: &Context<'_>) -> Result<Vec<Proof>, Refusal> {
        if m[0].0 == m[1].0 && m[1].0 == m[2].0 {
            Ok(vec![cx.proof("toy.E1", vec![("header", m[0].0.clone())])])
        } else {
            Err(refusal("HEADER", "header"))
        }
    }

    fn units(&self, m: &Model) -> BTreeMap<String, String> {
        m.1.clone()
    }

    fn decide(&self, c: &Conflict<'_, String, String>, cx: &Context<'_>) -> Verdict<String> {
        match c.key.as_str() {
            "bogus-rule" => {
                return Verdict::Accept("x".into(), cx.proof("toy.NOPE", Vec::new()));
            }
            "bogus-code" => return Verdict::Refuse(refusal("NOPE", "bogus-code")),
            _ => {}
        }
        let (Some(o), Some(t)) = (c.ours, c.theirs) else {
            return Verdict::NotApplicable;
        };
        if o == "bad" || t == "bad" {
            return Verdict::Refuse(refusal("R3", c.key));
        }
        if o == "fatal14" || t == "fatal14" {
            return Verdict::Refuse(refusal("R14", c.key));
        }
        match (o.parse::<i64>(), t.parse::<i64>()) {
            (Ok(a), Ok(b)) if c.base.is_some() => Verdict::Accept(
                a.max(b).to_string(),
                cx.proof("toy.M1", vec![("ours", o.clone()), ("theirs", t.clone())]),
            ),
            _ => Verdict::NotApplicable,
        }
    }

    fn prune(&self, d: &Draft<String, String>, cx: &Context<'_>) -> Vec<Removal<String>> {
        let mut out: Vec<Removal<String>> = d
            .iter()
            .filter(|(_, v)| v.as_str() == "orphan")
            .map(|(k, _)| Removal {
                key: k.clone(),
                proof: cx.proof("toy.P1", Vec::new()),
            })
            .collect();
        if d.contains_key(&"bogus-prune".to_string()) {
            out.push(Removal {
                key: "missing".into(),
                proof: cx.proof("toy.P1", Vec::new()),
            });
        }
        out
    }

    fn validate(&self, d: &Draft<String, String>, cx: &Context<'_>) -> Findings {
        let mut f = Findings::default();
        for (k, v) in d.iter() {
            if v.is_empty() {
                f.refusals.push(refusal("EMPTY", k));
            }
            if v == "soft" {
                f.refusals.push(refusal("R14", k));
            }
        }
        if d.iter().filter(|(_, v)| v.as_str() == "links").count() > 1 {
            f.refusals.push(refusal("R15", "links"));
        }
        if f.refusals.iter().all(|r| r.code != code("EMPTY")) {
            f.proofs.push(cx.proof("toy.V1", Vec::new()));
        }
        if d.contains_key(&"bogus-validate".to_string()) {
            f.proofs.push(cx.proof("toy.NOPE", Vec::new()));
        }
        f
    }

    fn serialize(&self, s: &Sealed<String, String>, m: [&Model; 3]) -> Vec<u8> {
        self.serialized.fetch_add(1, Ordering::SeqCst);
        let mut out = format!("{}\n", m[0].0);
        for (k, v) in s.iter() {
            out.push_str(&format!("{k}={v}\n"));
        }
        out.into_bytes()
    }
}

struct NoManifests;
impl Manifests for NoManifests {
    fn merged(&self, _: &str) -> ManifestResult {
        ManifestResult::Conflict
    }
}

fn file(lines: &[&str]) -> Vec<u8> {
    let mut s = String::from("toy 1\n");
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s.into_bytes()
}

fn try_merge(toy: &Toy, base: &[u8], ours: &[u8], theirs: &[u8]) -> Result<Report, Error> {
    let cx = Context::new(&NoManifests);
    let inputs = Inputs {
        base,
        ours,
        theirs,
        path: "toy.lock",
    };
    // Through the object-safe trait, as the CLI calls it.
    Format::merge(toy, &inputs, &cx)
}

fn merge(base: &[u8], ours: &[u8], theirs: &[u8]) -> Report {
    try_merge(&Toy::default(), base, ours, theirs).expect("no internal error")
}

fn clean(r: &Report) -> (&[u8], How) {
    match &r.outcome {
        Outcome::Clean(c) => (c.bytes(), c.how()),
        Outcome::Refused(x) => panic!("expected clean, got refusal {}: {:?}", x.code, x),
    }
}

fn refused(r: &Report) -> &Refusal {
    match &r.outcome {
        Outcome::Refused(x) => x,
        Outcome::Clean(_) => panic!("expected a refusal"),
    }
}

fn rules_applied(r: &Report) -> Vec<&str> {
    r.applied.iter().map(|a| a.rule).collect()
}

// ---- core.S1 ---------------------------------------------------------

#[test]
fn s1_accepts_identical_inputs_byte_for_byte_even_if_unparsable() {
    let junk: &[u8] = b"\xff not a toy file";
    let base = file(&["a=1"]);
    let r = merge(&base, junk, junk);
    assert_eq!(clean(&r), (junk, How::Unchanged(Side::Ours)));
    assert_eq!(rules_applied(&r), [S1]);
}

#[test]
fn s1_accepts_one_side_equal_to_base_and_returns_the_other_unchanged() {
    let base = file(&["a=1"]);
    let changed: &[u8] = b"\xff changed, unparsable";
    let r = merge(&base, &base, changed);
    assert_eq!(clean(&r), (changed, How::Unchanged(Side::Theirs)));
    let r = merge(&base, changed, &base);
    assert_eq!(clean(&r), (changed, How::Unchanged(Side::Ours)));
}

#[test]
fn s1_refuses_when_all_three_differ_and_one_is_unparsable() {
    // The neighboring case: S1 does not fire, so nothing returns unparsable
    // bytes; the parse refusal ends the merge.
    let r = merge(&file(&["a=1"]), b"\xff junk", &file(&["a=2"]));
    assert_eq!(refused(&r).code, code("PARSE"));
    assert!(!rules_applied(&r).contains(&S1));
}

// ---- core.B1 ---------------------------------------------------------

#[test]
fn b1_accepts_the_same_change_on_both_sides() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=7"]),
        &file(&["a=1", "b=7", "c=1"]),
    );
    // b identical in ours and theirs (B1); c added by theirs only (B3).
    assert_eq!(clean(&r).0, file(&["a=1", "b=7", "c=1"]));
    assert_eq!(rules_applied(&r), ["toy.E1", B1, B1, B3, "toy.V1"]);
}

#[test]
fn b1_accepts_removal_on_both_sides() {
    let r = merge(
        &file(&["a=1", "b=2"]),
        &file(&["a=1", "c=1"]),
        &file(&["a=1", "c=1", "d=1"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "c=1", "d=1"]));
    assert_eq!(r.applied[2].rule, B1);
    assert_eq!(r.applied[2].subjects, ["b"]);
}

#[test]
fn b1_refuses_different_additions_of_the_same_key() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=x"]),
        &file(&["a=1", "b=y"]),
    );
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(refused(&r).subjects, ["b"]);
}

// ---- core.B2 ---------------------------------------------------------

#[test]
fn b2_accepts_a_change_on_one_side_only() {
    let r = merge(
        &file(&["a=1", "b=1"]),
        &file(&["a=5", "b=1"]),
        &file(&["a=1", "b=9"]),
    );
    assert_eq!(clean(&r), (file(&["a=5", "b=9"]).as_slice(), How::Merged));
    assert_eq!(rules_applied(&r), ["toy.E1", B2, B2, "toy.V1"]);
}

#[test]
fn b2_refuses_when_both_sides_changed_to_different_units() {
    let r = merge(&file(&["a=x"]), &file(&["a=y"]), &file(&["a=z"]));
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(refused(&r).facts[0], ("base", "present".to_string()));
}

// ---- core.B3 ---------------------------------------------------------

#[test]
fn b3_accepts_an_add_on_one_side_only() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=2"]),
        &file(&["a=1", "c=3"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "b=2", "c=3"]));
    assert_eq!(rules_applied(&r), ["toy.E1", B1, B3, B3, "toy.V1"]);
}

#[test]
fn b3_refuses_when_both_sides_add_different_units() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=2"]),
        &file(&["a=1", "b=3"]),
    );
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(refused(&r).facts[0], ("base", "absent".to_string()));
}

// ---- core.B4 ---------------------------------------------------------

#[test]
fn b4_accepts_removal_of_an_unchanged_entry() {
    let r = merge(
        &file(&["a=1", "b=2"]),
        &file(&["a=1"]),
        &file(&["a=1", "b=2", "c=3"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "c=3"]));
    assert_eq!(rules_applied(&r), ["toy.E1", B1, B4, B3, "toy.V1"]);
}

#[test]
fn b4_refuses_removal_of_an_entry_the_other_side_changed() {
    let r = merge(
        &file(&["a=1", "b=2"]),
        &file(&["a=1"]),
        &file(&["a=1", "b=3"]),
    );
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(refused(&r).subjects, ["b"]);
    assert_eq!(refused(&r).facts[1], ("ours", "absent".to_string()));
}

// ---- core.UNPROVEN and the format's decide ------------------------------

#[test]
fn not_applicable_refuses_with_core_unproven() {
    // Both changed a non-integer: toy.M1 answers NotApplicable.
    let r = merge(&file(&["a=1"]), &file(&["a=x"]), &file(&["a=y"]));
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(refused(&r).code.to_string(), "core.UNPROVEN");
}

#[test]
fn a_format_rule_accepts_what_the_base_rules_do_not_and_names_itself() {
    // The neighboring accept for the case above: integers, so toy.M1 applies.
    let r = merge(&file(&["a=1"]), &file(&["a=3"]), &file(&["a=8"]));
    assert_eq!(clean(&r).0, file(&["a=8"]));
    assert_eq!(rules_applied(&r), ["toy.E1", "toy.M1", "toy.V1"]);
}

#[test]
fn a_format_refusal_keeps_its_own_code() {
    let r = merge(&file(&["a=1"]), &file(&["a=bad"]), &file(&["a=2"]));
    assert_eq!(refused(&r).code, code("R3"));
}

#[test]
fn a_non_fatal_refusal_from_decide_is_a_downgrade_and_the_key_is_unproven() {
    let r = merge(&file(&["a=1"]), &file(&["a=fatal14"]), &file(&["a=2"]));
    assert_eq!(refused(&r).code, UNPROVEN);
    assert_eq!(r.downgrades.len(), 1);
    assert_eq!(r.downgrades[0].code, code("R14"));
}

// ---- fatal and non-fatal refusals (issue #29) ------------------------------

#[test]
fn r15_style_refusal_is_fatal_and_never_a_downgrade() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=links"]),
        &file(&["a=1", "c=links"]),
    );
    assert_eq!(refused(&r).code, code("R15"));
    assert!(r.downgrades.is_empty());
}

#[test]
fn r15_style_check_accepts_a_single_links_unit() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=links"]),
        &file(&["a=1", "c=2"]),
    );
    assert!(matches!(r.outcome, Outcome::Clean(_)));
    assert!(r.downgrades.is_empty());
}

#[test]
fn a_non_fatal_validator_refusal_is_a_downgrade_and_the_merge_proceeds() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=soft"]),
        &file(&["a=1", "c=2"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "b=soft", "c=2"]));
    assert_eq!(r.downgrades.len(), 1);
    assert_eq!(r.downgrades[0].code, code("R14"));
}

#[test]
fn a_fatal_validator_refusal_ends_the_merge() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b="]),
        &file(&["a=1", "c=2"]),
    );
    assert_eq!(refused(&r).code, code("EMPTY"));
    assert!(r.downgrades.is_empty());
}

// ---- prune (issue #29) -------------------------------------------------

#[test]
fn prune_removes_a_unit_with_a_proof_and_records_it() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=orphan"]),
        &file(&["a=1", "c=2"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "c=2"]));
    assert_eq!(
        rules_applied(&r),
        ["toy.E1", B1, B3, B3, "toy.P1", "toy.V1"]
    );
    assert_eq!(r.applied[4].subjects, ["b"]);
}

#[test]
fn prune_leaves_a_neighboring_unit_alone() {
    let r = merge(
        &file(&["a=1"]),
        &file(&["a=1", "b=kept"]),
        &file(&["a=1", "c=2"]),
    );
    assert_eq!(clean(&r).0, file(&["a=1", "b=kept", "c=2"]));
    assert!(!rules_applied(&r).contains(&"toy.P1"));
}

#[test]
fn prune_of_a_key_not_in_the_draft_is_an_internal_error() {
    let ours = file(&["a=1", "bogus-prune=1"]);
    let e = try_merge(
        &Toy::default(),
        &file(&["a=1"]),
        &ours,
        &file(&["a=1", "c=2"]),
    )
    .unwrap_err();
    assert_eq!(
        e,
        Error::BadRemoval {
            format: TOY,
            key: "missing".into()
        }
    );
}

// ---- an unregistered rule or code is an internal error -----------------------

#[test]
fn an_unregistered_rule_in_decide_is_an_internal_error_never_a_merge() {
    let toy = Toy::default();
    let e = try_merge(
        &toy,
        &file(&["bogus-rule=1"]),
        &file(&["bogus-rule=2"]),
        &file(&["bogus-rule=3"]),
    )
    .unwrap_err();
    assert_eq!(
        e,
        Error::UnregisteredRule {
            format: TOY,
            rule: "toy.NOPE".into()
        }
    );
    assert_eq!(toy.serialized.load(Ordering::SeqCst), 0);
}

#[test]
fn an_unregistered_rule_in_validate_is_an_internal_error() {
    let toy = Toy::default();
    let e = try_merge(
        &toy,
        &file(&["a=1"]),
        &file(&["a=1", "bogus-validate=1"]),
        &file(&["a=1", "c=2"]),
    )
    .unwrap_err();
    assert!(matches!(e, Error::UnregisteredRule { .. }));
    assert_eq!(toy.serialized.load(Ordering::SeqCst), 0);
}

#[test]
fn an_unregistered_code_is_an_internal_error() {
    let e = try_merge(
        &Toy::default(),
        &file(&["bogus-code=1"]),
        &file(&["bogus-code=2"]),
        &file(&["bogus-code=3"]),
    )
    .unwrap_err();
    assert_eq!(
        e,
        Error::UnregisteredCode {
            format: TOY,
            code: "toy.NOPE".into()
        }
    );
}

struct CoreRuleFormat(Toy);
impl Pipeline for CoreRuleFormat {
    type Model = Model;
    type Key = String;
    type Unit = String;
    fn id(&self) -> FormatId {
        TOY
    }
    fn rules(&self) -> &'static [RuleDef] {
        &[RuleDef {
            id: "core.B1",
            summary: "forged",
            adr: "0000",
        }]
    }
    fn codes(&self) -> &'static [CodeDef] {
        CODES
    }
    fn matches(&self, _: &str) -> bool {
        true
    }
    fn parse(&self, r: Role, b: &[u8]) -> Result<Model, Refusal> {
        self.0.parse(r, b)
    }
    fn envelope(&self, _: [&Model; 3], _: &Context<'_>) -> Result<Vec<Proof>, Refusal> {
        Ok(Vec::new())
    }
    fn units(&self, m: &Model) -> BTreeMap<String, String> {
        self.0.units(m)
    }
    fn decide(&self, _: &Conflict<'_, String, String>, _: &Context<'_>) -> Verdict<String> {
        Verdict::NotApplicable
    }
    fn validate(&self, _: &Draft<String, String>, _: &Context<'_>) -> Findings {
        Findings::default()
    }
    fn serialize(&self, s: &Sealed<String, String>, m: [&Model; 3]) -> Vec<u8> {
        self.0.serialize(s, m)
    }
}

#[test]
fn a_format_may_not_register_an_engine_rule() {
    let f = CoreRuleFormat(Toy::default());
    let cx = Context::new(&NoManifests);
    let (b, o, t) = (file(&["a=1"]), file(&["a=2"]), file(&["a=3"]));
    let e = engine::run(
        &f,
        &Inputs {
            base: &b,
            ours: &o,
            theirs: &t,
            path: "toy.lock",
        },
        &cx,
    )
    .unwrap_err();
    assert!(matches!(e, Error::BadRegistry { .. }));
}

// ---- serialize is reachable only through Sealed ---------------------------

#[test]
fn serialize_runs_only_after_every_check_passed() {
    let toy = Toy::default();
    // Refused in parse, envelope, keys and validators: never serialized.
    let bad_header = b"toy 2\na=1\n".to_vec();
    let cases: [(Vec<u8>, Vec<u8>, Vec<u8>); 4] = [
        (file(&["a=1"]), b"\xff".to_vec(), file(&["a=2"])),
        (file(&["a=1"]), bad_header, file(&["a=2"])),
        (file(&["a=1"]), file(&["a=x"]), file(&["a=y"])),
        (file(&["a=1"]), file(&["b="]), file(&["a=2"])),
    ];
    for (b, o, t) in &cases {
        assert!(matches!(
            try_merge(&toy, b, o, t).unwrap().outcome,
            Outcome::Refused(_)
        ));
    }
    assert_eq!(toy.serialized.load(Ordering::SeqCst), 0);
    // A clean merge serializes exactly once.
    let r = try_merge(
        &toy,
        &file(&["a=1"]),
        &file(&["a=2"]),
        &file(&["a=1", "b=1"]),
    )
    .unwrap();
    assert!(matches!(r.outcome, Outcome::Clean(_)));
    assert_eq!(toy.serialized.load(Ordering::SeqCst), 1);
}

#[test]
fn s1_never_calls_serialize() {
    let toy = Toy::default();
    let b = file(&["a=1"]);
    try_merge(&toy, &b, &b, &file(&["a=2"])).unwrap();
    assert_eq!(toy.serialized.load(Ordering::SeqCst), 0);
}

// ---- deterministic refusal order -----------------------------------------

#[test]
fn refusals_come_in_a_fixed_order_and_the_primary_is_the_first() {
    let base = file(&["a=1", "b=1", "c=1"]);
    let ours = file(&["a=x", "b=x", "c=x"]);
    let theirs = file(&["a=y", "b=y", "c=y"]);
    let r1 = merge(&base, &ours, &theirs);
    let r2 = merge(&base, &ours, &theirs);
    assert_eq!(r1, r2);
    assert_eq!(refused(&r1).subjects, ["a"]);
    let others: Vec<_> = r1
        .also_refused
        .iter()
        .map(|r| r.subjects[0].as_str())
        .collect();
    assert_eq!(others, ["b", "c"]);
}

#[test]
fn parse_refusals_precede_envelope_and_follow_role_order() {
    // Ours and theirs unparsable: both reported, ours first.
    let r = merge(&file(&["a=1"]), b"\xff", b"\xfe");
    assert_eq!(refused(&r).subjects, ["Ours"]);
    assert_eq!(r.also_refused[0].subjects, ["Theirs"]);
    // Parsable but headers disagree: the envelope refuses before any key.
    let r = merge(&file(&["a=x"]), b"toy 2\na=y\n", &file(&["a=z"]));
    assert_eq!(refused(&r).code, code("HEADER"));
    assert!(r.also_refused.is_empty());
}

#[test]
fn key_refusals_precede_validator_refusals() {
    // A key refusal ends the run before validators run.
    let r = merge(&file(&["a=1"]), &file(&["a=x", "b="]), &file(&["a=y"]));
    assert_eq!(refused(&r).code, UNPROVEN);
    assert!(r.also_refused.is_empty());
}

// ---- registry sanity of core ---------------------------------------------

#[test]
fn core_rules_and_codes_are_registered_with_the_core_prefix() {
    let ids: Vec<_> = engine::CORE_RULES.iter().map(|r| r.id).collect();
    assert_eq!(ids, [S1, B1, B2, B3, B4]);
    assert!(
        engine::CORE_CODES
            .iter()
            .all(|c| c.code.format == FormatId("core") && c.fatal)
    );
    assert_eq!(engine::CORE_CODES[0].code, UNPROVEN);
}
