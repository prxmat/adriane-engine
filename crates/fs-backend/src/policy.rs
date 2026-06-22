//! Per-path permission policy (ADR 0024). A pure, DB-free resolver: the control
//! plane compiles its owner-only rule table into a [`StaticPathPolicy`] handed to
//! the engine. Fail-closed: an unmatched path (and a policy with no rules) resolves
//! to [`FsPermVerb::Read`], so nothing is silently writable.

use crate::types::FsPermVerb;

/// Resolves a normalized path to a permission verb.
pub trait PathPolicy: Send + Sync {
    fn resolve(&self, path: &str) -> FsPermVerb;
}

/// A compiled rule: a glob pattern (`*` matches within a segment, `**` across
/// segments) and the verb it grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRule {
    pub glob: String,
    pub verb: FsPermVerb,
}

/// A static set of path rules. Resolution = **most-specific glob wins**; on a
/// specificity tie, the **most restrictive** verb wins (fail-closed). No match →
/// [`FsPermVerb::Read`].
#[derive(Clone, Debug, Default)]
pub struct StaticPathPolicy {
    rules: Vec<PathRule>,
}

impl StaticPathPolicy {
    /// Fail-closed read-only everywhere (no rules) — the default for a run with no
    /// policy (the chosen default, ADR 0024). Writes require an explicit rule.
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn with_rules(rules: Vec<PathRule>) -> Self {
        StaticPathPolicy { rules }
    }

    pub fn push(&mut self, glob: impl Into<String>, verb: FsPermVerb) -> &mut Self {
        self.rules.push(PathRule {
            glob: glob.into(),
            verb,
        });
        self
    }
}

impl PathPolicy for StaticPathPolicy {
    fn resolve(&self, path: &str) -> FsPermVerb {
        let mut best: Option<(usize, FsPermVerb)> = None;
        for rule in &self.rules {
            if glob_match(&rule.glob, path) {
                let specificity = literal_specificity(&rule.glob);
                let better = match best {
                    None => true,
                    Some((best_spec, best_verb)) => {
                        specificity > best_spec
                            || (specificity == best_spec && rule.verb.rank() > best_verb.rank())
                    }
                };
                if better {
                    best = Some((specificity, rule.verb));
                }
            }
        }
        best.map(|(_, verb)| verb).unwrap_or(FsPermVerb::Read)
    }
}

/// Specificity = count of literal (non-wildcard, non-separator) characters. `**`
/// is least specific; a fully literal path is most specific.
fn literal_specificity(glob: &str) -> usize {
    glob.chars().filter(|c| *c != '*' && *c != '/').count()
}

/// Match a glob against a normalized path. `**` matches zero or more whole path
/// segments; a non-`**` pattern segment matches exactly one path segment, with
/// `*` inside it matching any run of non-`/` characters.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    match_segments(&pat, &segs)
}

fn match_segments(pat: &[&str], segs: &[&str]) -> bool {
    match pat.first() {
        None => segs.is_empty(),
        Some(&"**") => {
            // `**` consumes 0..=segs.len() leading path segments.
            (0..=segs.len()).any(|k| match_segments(&pat[1..], &segs[k..]))
        }
        Some(head) => match segs.first() {
            Some(seg) if segment_match(head, seg) => match_segments(&pat[1..], &segs[1..]),
            _ => false,
        },
    }
}

/// Match a single segment pattern (with `*` = any run of chars within the segment)
/// against a single path segment.
fn segment_match(pat: &str, seg: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    let seg: Vec<char> = seg.chars().collect();
    // Two-pointer wildcard match with backtracking on the last `*`.
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star_pi, mut star_si): (Option<usize>, usize) = (None, 0);
    while si < seg.len() {
        if pi < pat.len() && pat[pi] == '*' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if pi < pat.len() && pat[pi] == seg[si] {
            pi += 1;
            si += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_stays_within_a_segment() {
        assert!(glob_match("scratch/*.md", "scratch/notes.md"));
        assert!(!glob_match("scratch/*.md", "scratch/sub/notes.md"));
        assert!(glob_match("*.txt", "a.txt"));
        assert!(!glob_match("*.txt", "a/b.txt"));
    }

    #[test]
    fn double_star_crosses_segments() {
        assert!(glob_match("**", "a/b/c.txt"));
        assert!(glob_match("scratch/**", "scratch/a/b.txt"));
        assert!(glob_match("scratch/**", "scratch/x.md"));
        assert!(!glob_match("scratch/**", "other/x.md"));
        assert!(glob_match("src/**/*.rs", "src/a/b/c.rs"));
        assert!(glob_match("src/**/*.rs", "src/c.rs"));
    }

    #[test]
    fn no_rules_is_read_only_everywhere() {
        let policy = StaticPathPolicy::read_only();
        assert_eq!(policy.resolve("anything/at/all"), FsPermVerb::Read);
        assert!(!policy.resolve("anything").can_write());
    }

    #[test]
    fn most_specific_glob_wins() {
        let policy = StaticPathPolicy::with_rules(vec![
            PathRule {
                glob: "**".to_owned(),
                verb: FsPermVerb::Read,
            },
            PathRule {
                glob: "scratch/**".to_owned(),
                verb: FsPermVerb::Write,
            },
            PathRule {
                glob: "scratch/secret.txt".to_owned(),
                verb: FsPermVerb::Deny,
            },
        ]);
        assert_eq!(policy.resolve("docs/readme.md"), FsPermVerb::Read);
        assert_eq!(policy.resolve("scratch/work.txt"), FsPermVerb::Write);
        assert_eq!(policy.resolve("scratch/secret.txt"), FsPermVerb::Deny);
    }

    #[test]
    fn restrictive_verb_wins_on_specificity_tie() {
        // Two equally-specific globs over the same path → the more restrictive wins.
        let policy = StaticPathPolicy::with_rules(vec![
            PathRule {
                glob: "data/*.json".to_owned(),
                verb: FsPermVerb::Write,
            },
            PathRule {
                glob: "data/*.json".to_owned(),
                verb: FsPermVerb::Read,
            },
        ]);
        assert_eq!(policy.resolve("data/x.json"), FsPermVerb::Read);
    }
}
