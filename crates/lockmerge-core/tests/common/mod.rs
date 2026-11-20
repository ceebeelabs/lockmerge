//! The toy format shared by the engine tests and the fixture tests, through
//! the public API only, as a format crate would use it.
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

use lockmerge_core::{
    CodeDef, Conflict, Context, Draft, Findings, FormatId, Pipeline, Proof, ReasonCode, Refusal,
    Removal, Role, RuleDef, Sealed, Verdict,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const TOY: FormatId = FormatId("toy");
pub const fn code(c: &'static str) -> ReasonCode {
    ReasonCode::new(TOY, c)
}

pub static RULES: &[RuleDef] = &[
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
pub static CODES: &[CodeDef] = &[
    def("PARSE", true),
    def("HEADER", true),
    def("R3", true),
    def("R14", false),
    def("R15", true),
    def("EMPTY", true),
];

#[derive(Default)]
pub struct Toy {
    pub serialized: AtomicUsize,
}

pub type Model = (String, BTreeMap<String, String>);

pub fn refusal(c: &'static str, subject: &str) -> Refusal {
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
