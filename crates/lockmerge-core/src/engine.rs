//! The three-way engine. Every accepted byte goes through [`run`].

use crate::{
    Applied, Clean, CodeDef, Conflict, Context, Draft, Error, FormatId, How, Inputs, Outcome,
    Pipeline, Proof, ReasonCode, Refusal, Report, Role, RuleDef, Side, Verdict,
};
use std::collections::BTreeSet;

const CORE: FormatId = FormatId("core");

/// The default refusal: no rule accepted the case.
pub const UNPROVEN: ReasonCode = ReasonCode::new(CORE, "UNPROVEN");

/// Unchanged input: two inputs are byte-identical or one equals base. The
/// bytes are returned as they are, never re-serialized.
pub const S1: &str = "core.S1";
/// Identical on both sides (including removed on both).
pub const B1: &str = "core.B1";
/// One-sided change: one side equals base, the other changed the entry.
pub const B2: &str = "core.B2";
/// One-sided add: the entry is not in base and only one side added it.
pub const B3: &str = "core.B3";
/// Removal of an unchanged entry: one side removed it, the other kept it
/// equal to base.
pub const B4: &str = "core.B4";

/// The rules of the engine itself. A format cannot name them in a `Proof`.
pub const CORE_RULES: &[RuleDef] = &[
    RuleDef {
        id: S1,
        summary: "Two inputs are identical, or one equals the base: return an input unchanged.",
        adr: "0012",
    },
    RuleDef {
        id: B1,
        summary: "An entry is identical in ours and theirs.",
        adr: "0012",
    },
    RuleDef {
        id: B2,
        summary: "An entry changed on one side only: take the changed one.",
        adr: "0012",
    },
    RuleDef {
        id: B3,
        summary: "An entry was added on one side only: take it.",
        adr: "0012",
    },
    RuleDef {
        id: B4,
        summary: "An entry was removed on one side and unchanged on the other: drop it.",
        adr: "0012",
    },
];

/// The reason codes of the engine itself.
pub const CORE_CODES: &[CodeDef] = &[CodeDef {
    code: UNPROVEN,
    title: "No rule could prove this change safe",
    explain: "Both branches changed {subjects} in ways no rule can combine.",
    next_step: "Resolve this entry by hand in the conflict markers.",
    fatal: true,
}];

/// Merges three inputs with `p`. Refusal is the default: only what a rule
/// accepts, with a registered proof, reaches the output.
///
/// The order of refusals is fixed: parse (base, ours, theirs), envelope, keys
/// in sorted order, then validators in the order the format returned them.
/// The first fatal refusal is the primary one.
pub fn run<P: Pipeline>(p: &P, inputs: &Inputs<'_>, cx: &Context<'_>) -> Result<Report, Error> {
    let format = Pipeline::id(p);
    check_registry(format, Pipeline::rules(p), Pipeline::codes(p))?;
    let mut run = Run::new(p, format);

    // core.S1: unchanged inputs are returned byte for byte, parsed or not.
    if let Some(side) = unchanged(inputs) {
        let bytes = match side {
            Side::Ours => inputs.ours,
            Side::Theirs => inputs.theirs,
        };
        run.applied.push(applied(S1, vec![inputs.path.to_string()]));
        return Ok(run.report(Outcome::Clean(Clean::new(
            bytes.to_vec(),
            How::Unchanged(side),
        ))));
    }

    // Parse all three, in a fixed order, so every parse refusal is reported.
    let mut models = Vec::new();
    for (role, bytes) in [
        (Role::Base, inputs.base),
        (Role::Ours, inputs.ours),
        (Role::Theirs, inputs.theirs),
    ] {
        match p.parse(role, bytes) {
            Ok(m) => models.push(m),
            Err(r) => {
                run.check_code(&r)?;
                run.fatal.push(r);
            }
        }
    }
    let [Some(base_m), Some(ours_m), Some(theirs_m)] = take3(models) else {
        return Ok(run.refused());
    };
    let ms = [&base_m, &ours_m, &theirs_m];

    match p.envelope(ms, cx) {
        Ok(proofs) => run.record_proofs(proofs, vec![inputs.path.to_string()])?,
        Err(r) => {
            run.check_code(&r)?;
            run.fatal.push(r);
            return Ok(run.refused());
        }
    }

    // Per-key pass, keys in sorted order.
    let (bu, ou, tu) = (p.units(&base_m), p.units(&ours_m), p.units(&theirs_m));
    let keys: BTreeSet<&P::Key> = bu.keys().chain(ou.keys()).chain(tu.keys()).collect();
    let mut draft: Draft<P::Key, P::Unit> = Draft::new();
    for key in keys {
        let (b, o, t) = (bu.get(key), ou.get(key), tu.get(key));
        let subject = vec![key.to_string()];
        match base_rule(b, o, t) {
            Some((rule, keep)) => {
                if let Some(unit) = keep {
                    draft.insert(key.clone(), unit.clone(), Proof::new(rule, Vec::new()));
                }
                run.applied.push(applied(rule, subject));
            }
            None => {
                let conflict = Conflict {
                    key,
                    base: b,
                    ours: o,
                    theirs: t,
                };
                match p.decide(&conflict, cx) {
                    Verdict::Accept(unit, proof) => {
                        run.check_proof(&proof)?;
                        run.applied.push(applied(proof.rule(), subject));
                        draft.insert(key.clone(), unit, proof);
                    }
                    Verdict::Refuse(r) => {
                        run.check_code(&r)?;
                        if run.is_fatal(&r.code) {
                            run.fatal.push(r);
                        } else {
                            run.downgrades.push(r);
                            run.fatal.push(unproven(key, b, o, t));
                        }
                    }
                    Verdict::NotApplicable => run.fatal.push(unproven(key, b, o, t)),
                }
            }
        }
    }
    if !run.fatal.is_empty() {
        return Ok(run.refused());
    }

    // Removals with proofs, then whole-set checks on the pruned draft.
    for removal in p.prune(&draft, cx) {
        run.check_proof(&removal.proof)?;
        if !draft.remove(&removal.key) {
            return Err(Error::BadRemoval {
                format,
                key: removal.key.to_string(),
            });
        }
        run.applied
            .push(applied(removal.proof.rule(), vec![removal.key.to_string()]));
    }

    let findings = p.validate(&draft, cx);
    let mut proofs = Vec::new();
    for r in findings.refusals {
        run.check_code(&r)?;
        if run.is_fatal(&r.code) {
            run.fatal.push(r);
        } else {
            run.downgrades.push(r);
        }
    }
    for proof in findings.proofs {
        run.check_proof(&proof)?;
        proofs.push(proof);
    }
    if !run.fatal.is_empty() {
        return Ok(run.refused());
    }
    for proof in proofs {
        run.applied
            .push(applied(proof.rule(), vec![inputs.path.to_string()]));
    }

    let bytes = p.serialize(&draft.seal(), ms);
    Ok(run.report(Outcome::Clean(Clean::new(bytes, How::Merged))))
}

struct Run<'a, P: Pipeline> {
    p: &'a P,
    format: FormatId,
    applied: Vec<Applied>,
    downgrades: Vec<Refusal>,
    fatal: Vec<Refusal>,
}

impl<'a, P: Pipeline> Run<'a, P> {
    fn new(p: &'a P, format: FormatId) -> Self {
        Self {
            p,
            format,
            applied: Vec::new(),
            downgrades: Vec::new(),
            fatal: Vec::new(),
        }
    }

    fn check_proof(&self, proof: &Proof) -> Result<(), Error> {
        if Pipeline::rules(self.p).iter().any(|r| r.id == proof.rule()) {
            Ok(())
        } else {
            Err(Error::UnregisteredRule {
                format: self.format,
                rule: proof.rule().to_string(),
            })
        }
    }

    fn record_proofs(&mut self, proofs: Vec<Proof>, subjects: Vec<String>) -> Result<(), Error> {
        for proof in &proofs {
            self.check_proof(proof)?;
        }
        for proof in proofs {
            self.applied.push(applied(proof.rule(), subjects.clone()));
        }
        Ok(())
    }

    fn check_code(&self, r: &Refusal) -> Result<(), Error> {
        if r.code == UNPROVEN || Pipeline::codes(self.p).iter().any(|c| c.code == r.code) {
            Ok(())
        } else {
            Err(Error::UnregisteredCode {
                format: self.format,
                code: r.code.to_string(),
            })
        }
    }

    /// Fatal unless the format's table says the code is non-fatal. Only
    /// registered codes get here (see `check_code`).
    fn is_fatal(&self, code: &ReasonCode) -> bool {
        Pipeline::codes(self.p)
            .iter()
            .find(|c| c.code == *code)
            .is_none_or(|c| c.fatal)
    }

    fn report(self, outcome: Outcome) -> Report {
        Report {
            format: self.format,
            outcome,
            applied: self.applied,
            downgrades: self.downgrades,
            also_refused: Vec::new(),
        }
    }

    fn refused(mut self) -> Report {
        let mut fatal = std::mem::take(&mut self.fatal).into_iter();
        let primary = fatal
            .next()
            .expect("refused() is called only with a fatal refusal");
        let mut report = self.report(Outcome::Refused(primary));
        report.also_refused = fatal.collect();
        report
    }
}

fn applied(rule: &'static str, subjects: Vec<String>) -> Applied {
    Applied { rule, subjects }
}

fn take3<M>(v: Vec<M>) -> [Option<M>; 3] {
    let mut it = v.into_iter();
    [it.next(), it.next(), it.next()]
}

/// S1: which input to return unchanged, if any. Ours and theirs identical:
/// ours. Ours equals base: theirs. Theirs equals base: ours.
fn unchanged(i: &Inputs<'_>) -> Option<Side> {
    if i.ours == i.theirs || i.theirs == i.base {
        Some(Side::Ours)
    } else if i.ours == i.base {
        Some(Side::Theirs)
    } else {
        None
    }
}

/// The base rules for one key. `Some((rule, unit))` is settled, with `None`
/// meaning the key is dropped. `None` means the format must decide.
fn base_rule<'a, U: Eq>(
    b: Option<&'a U>,
    o: Option<&'a U>,
    t: Option<&'a U>,
) -> Option<(&'static str, Option<&'a U>)> {
    if o == t {
        return Some((B1, o));
    }
    // From here ours and theirs differ. If one side equals base, the other
    // side is the one that changed.
    let changed = if b == o {
        t
    } else if b == t {
        o
    } else {
        return None;
    };
    Some(match (b, changed) {
        (Some(_), Some(_)) => (B2, changed),
        (None, Some(_)) => (B3, changed),
        (Some(_), None) => (B4, None),
        // Base, ours and theirs all absent cannot differ.
        (None, None) => return None,
    })
}

fn unproven<K: std::fmt::Display, U>(
    key: &K,
    b: Option<&U>,
    o: Option<&U>,
    t: Option<&U>,
) -> Refusal {
    let state = |u: Option<&U>| if u.is_some() { "present" } else { "absent" };
    Refusal::new(
        UNPROVEN,
        vec![key.to_string()],
        vec![
            ("base", state(b).to_string()),
            ("ours", state(o).to_string()),
            ("theirs", state(t).to_string()),
        ],
    )
}

/// Rule ids and codes belong to the format: a `core.*` id would let a format
/// forge an engine rule, and duplicates make tables ambiguous.
fn check_registry(format: FormatId, rules: &[RuleDef], codes: &[CodeDef]) -> Result<(), Error> {
    let bad = |detail: String| Err(Error::BadRegistry { format, detail });
    let prefix = format!("{format}.");
    let mut seen = BTreeSet::new();
    for r in rules {
        if !r.id.starts_with(&prefix) {
            return bad(format!("rule {} does not start with {prefix}", r.id));
        }
        if !seen.insert(r.id) {
            return bad(format!("rule {} is registered twice", r.id));
        }
    }
    let mut seen = BTreeSet::new();
    for c in codes {
        if c.code.format != format {
            return bad(format!("code {} belongs to another format", c.code));
        }
        if !seen.insert(c.code) {
            return bad(format!("code {} is registered twice", c.code));
        }
    }
    Ok(())
}
