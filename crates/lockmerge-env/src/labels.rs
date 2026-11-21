//! Reading the labels git passes to a merge driver (ADR 0010 decisions 2, 3
//! and 5). Pure: no processes, no environment.
//!
//! Git does not document these formats as stable, so only the exact shapes
//! probed on real git are accepted. Everything else is refused with a reason.
//!
//! | Operation | `%S` | `%Y` |
//! | --- | --- | --- |
//! | `git merge` | short sha of the merge base | branch name, the value of `GITHEAD_<sha>` |
//! | `git cherry-pick`, `git rebase` (merge backend) | `parent of <short> (<subject>)` | `<short> (<subject>)` |
//!
//! `git revert` (`%S` and `%Y` swapped roles) and the `--apply` rebase backend
//! (`constructed fake ancestor`, or `constructed merge base` in newer git) are not identified. `%X` must be `HEAD`.

use lockmerge_core::Why;

/// What the labels say, before any object is looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Labels {
    /// `git merge`: `base` is the abbreviated sha of the merge base; `theirs`
    /// is the full sha from the matching `GITHEAD_` variable.
    Merge { base: String, theirs: String },
    /// `git cherry-pick` or `git rebase` (merge backend): `theirs` is the
    /// abbreviated sha of the commit being applied. Its first parent is the
    /// base; the caller checks that it has exactly one.
    Pick { theirs: String },
}

fn why(text: impl Into<String>) -> Why {
    Why(text.into())
}

/// Lowercase hex of an abbreviated sha: at least 4 digits (git's minimum), at
/// most 64 (a SHA-256 object name).
fn is_abbrev(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A full object name: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
pub(crate) fn is_full_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && is_abbrev(s)
}

/// Splits `<hex> (<text>)` into the hex. `text` must be non-empty and
/// contain no line break.
fn pick_label_sha(label: &str) -> Option<&str> {
    let (sha, rest) = label.split_once(' ')?;
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    if inner.is_empty() || inner.contains(['\n', '\r']) || !is_abbrev(sha) {
        return None;
    }
    Some(sha)
}

/// Interprets `%S`, `%X`, `%Y` and the `GITHEAD_<full sha>=<value>`
/// variables (given as `(full sha, value)` pairs).
///
/// Refuses (with the reason) every shape that is not exactly one of the two
/// known ones.
pub fn parse_labels(
    base: &str,
    ours: &str,
    theirs: &str,
    githead: &[(String, String)],
) -> Result<Labels, Why> {
    if ours != "HEAD" {
        return Err(why(format!("unknown ours label {ours:?}, expected HEAD")));
    }
    if let Some(sha) = pick_label_sha(theirs) {
        // cherry-pick and rebase (merge backend)
        return if base.strip_prefix("parent of ") == Some(theirs) {
            Ok(Labels::Pick {
                theirs: sha.to_string(),
            })
        } else {
            Err(why(format!(
                "base label {base:?} is not \"parent of\" the commit named by {theirs:?}"
            )))
        };
    }
    if base.starts_with("parent of ") || pick_label_sha(base).is_some() {
        // A revert swaps the roles of %S and %Y, and git revert is not
        // established as identifiable (ADR 0010 Consequences).
        return Err(why(
            "labels of an operation that is not identified (revert)".to_string(),
        ));
    }
    if is_abbrev(base) {
        // git merge. Exactly one GITHEAD_ variable may exist: an octopus
        // merge sets several, and it is not identified.
        return match githead {
            [(sha, value)] if value == theirs && is_full_sha(sha) => Ok(Labels::Merge {
                base: base.to_string(),
                theirs: sha.clone(),
            }),
            [] => Err(why("no GITHEAD_ variable: not a git merge")),
            [_] => Err(why("the GITHEAD_ variable does not match the theirs label")),
            _ => Err(why("several GITHEAD_ variables: not a two-sided merge")),
        };
    }
    Err(why(format!("unknown base label {base:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "f96df549c503814c023d389585f4d22e372df927";

    fn gh(sha: &str, v: &str) -> Vec<(String, String)> {
        vec![(sha.to_string(), v.to_string())]
    }

    #[test]
    fn merge_is_accepted() {
        assert_eq!(
            parse_labels("25b44bf", "HEAD", "side", &gh(FULL, "side")),
            Ok(Labels::Merge {
                base: "25b44bf".into(),
                theirs: FULL.into()
            })
        );
    }

    #[test]
    fn merge_neighbors_are_refused() {
        // no variable, a variable for another name, two variables
        assert!(parse_labels("25b44bf", "HEAD", "side", &[]).is_err());
        assert!(parse_labels("25b44bf", "HEAD", "side", &gh(FULL, "other")).is_err());
        let two = vec![(FULL.to_string(), "side".to_string()); 2];
        assert!(parse_labels("25b44bf", "HEAD", "side", &two).is_err());
        // a variable whose name is not a full sha
        assert!(parse_labels("25b44bf", "HEAD", "side", &gh("f96df54", "side")).is_err());
        // ours is not HEAD
        assert!(parse_labels("25b44bf", "main", "side", &gh(FULL, "side")).is_err());
        // base is not an abbreviated sha: a virtual ancestor, or uppercase hex
        assert!(
            parse_labels("merged common ancestors", "HEAD", "side", &gh(FULL, "side")).is_err()
        );
        assert!(parse_labels("25B44BF", "HEAD", "side", &gh(FULL, "side")).is_err());
        assert!(parse_labels("25b", "HEAD", "side", &gh(FULL, "side")).is_err());
    }

    #[test]
    fn pick_is_accepted() {
        assert_eq!(
            parse_labels(
                "parent of f96df54 (side commit)",
                "HEAD",
                "f96df54 (side commit)",
                &[]
            ),
            Ok(Labels::Pick {
                theirs: "f96df54".into()
            })
        );
    }

    #[test]
    fn pick_neighbors_are_refused() {
        let y = "f96df54 (side commit)";
        // the base label names another commit or another subject
        assert!(parse_labels("parent of 1234567 (side commit)", "HEAD", y, &[]).is_err());
        assert!(parse_labels("parent of f96df54 (other)", "HEAD", y, &[]).is_err());
        // the base label lacks "parent of"
        assert!(parse_labels(y, "HEAD", y, &[]).is_err());
        // theirs has the wrong shape
        for bad in [
            "f96df54",
            "f96df54 ()",
            "f96df54 side",
            "xyz1234 (s)",
            "f96 (s)",
        ] {
            let s = format!("parent of {bad}");
            assert!(parse_labels(&s, "HEAD", bad, &[]).is_err(), "{bad}");
        }
        // ours is not HEAD
        assert!(parse_labels(&format!("parent of {y}"), "main", y, &[]).is_err());
    }

    #[test]
    fn revert_apply_backend_and_unknown_shapes_are_refused() {
        // git revert: %S is the reverted commit, %Y its parent
        assert!(
            parse_labels(
                "f96df54 (side commit)",
                "HEAD",
                "parent of f96df54 (side commit)",
                &[]
            )
            .is_err()
        );
        // git rebase --apply
        assert!(parse_labels("constructed fake ancestor", "HEAD", "side commit", &[]).is_err());
        // the same backend in newer git words the base label differently
        assert!(parse_labels("constructed merge base", "HEAD", "side commit", &[]).is_err());
        // standalone, merge-file and merge-tree style labels
        assert!(parse_labels("", "", "", &[]).is_err());
        assert!(parse_labels("base", "ours", "theirs", &[]).is_err());
        assert!(parse_labels("base", "HEAD", "theirs", &gh(FULL, "theirs")).is_err());
    }

    #[test]
    fn a_pick_label_wins_over_githead_only_when_complete() {
        // a stray GITHEAD_ variable does not turn a malformed pick into a merge
        assert!(
            parse_labels(
                "parent of f96df54 (s)",
                "HEAD",
                "f96df54 (t)",
                &gh(FULL, "f96df54 (t)")
            )
            .is_err()
        );
    }
}
