//! What `wait` with `pr` needs of a pull request: a reference to one, its
//! verdict (the checks counted by state, the failing ones by name,
//! `mergeable`, the merge queue, who merged it) and whether a verdict meets
//! what's waited for. The reads themselves are the forge crate's
//! ([`crate::forges::pr_verdicts`]); the loop is MCP's.

use arugula_proto::forge::{Check, CheckState, Item, ItemState};
use serde::Serialize;

/// A pull request on GitHub (or GitHub Enterprise's `host`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRef {
    pub host: String,
    pub repo: String,
    pub number: u64,
}

impl PrRef {
    /// `OWNER/REPO#N` or a pull request's link (`https://HOST/OWNER/REPO/pull/N`).
    pub fn parse(s: &str) -> Option<PrRef> {
        let s = s.trim();
        let github = |repo: &str, n: &str, host: &str| {
            let (owner, name) = repo.split_once('/')?;
            let ok = |p: &str| !p.is_empty() && !p.contains(['/', '#', ' ']);
            (ok(owner) && ok(name)).then(|| PrRef {
                host: host.to_ascii_lowercase(),
                repo: repo.to_owned(),
                number: n.parse().unwrap_or(0),
            })
        };
        let found = if let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) {
            let rest = rest.split(['?', '#']).next().unwrap_or(rest);
            let p: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
            match p.as_slice() {
                [host, owner, name, "pull", n, ..] => github(&format!("{owner}/{name}"), n, host),
                _ => None,
            }
        } else {
            let (repo, n) = s.split_once('#')?;
            github(repo, n, "github.com")
        };
        found.filter(|p| p.number > 0)
    }
}

impl std::fmt::Display for PrRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// What a wait on a pull request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrWant {
    /// No check is queued or running.
    ChecksDone,
    /// A check failed.
    ChecksFailed,
    /// Checks green, and it can merge cleanly.
    Ready,
    Merged,
    /// Closed, or merged.
    Closed,
}

/// The checks, counted by state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CheckCounts {
    pub success: u32,
    pub failure: u32,
    pub running: u32,
    pub queued: u32,
    pub skipped: u32,
    /// Cancelled, neutral, needing an action, or manual.
    pub other: u32,
}

/// One pull request's answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PrVerdict {
    pub pr: String,
    pub url: String,
    pub state: ItemState,
    pub draft: bool,
    pub checks: CheckCounts,
    /// The names of the checks that failed.
    pub failing: Vec<String>,
    /// `None` while GitHub works it out.
    pub mergeable: Option<bool>,
    /// Branch protection holds the merge (a review, say).
    pub blocked: bool,
    /// In the branch's merge queue now; `None` when it has none.
    pub queued: Option<bool>,
    pub merged_by: Option<String>,
    pub merged_at_ms: Option<i64>,
    /// Whether it's what the wait is for.
    pub met: bool,
    /// Why it can't be waited on (it isn't there, or isn't yours to see).
    pub error: Option<String>,
    /// Checks that count (not skipped, neutral, manual or allowed to fail),
    /// and how many of them aren't done: the rollup's own rule.
    #[serde(skip)]
    counted: u32,
    #[serde(skip)]
    pending: u32,
}

impl PrVerdict {
    /// A pull request that can't be read: it answers the same whether it
    /// doesn't exist or the person can't see it.
    pub fn gone(pr: &PrRef) -> PrVerdict {
        PrVerdict {
            pr: pr.to_string(),
            url: String::new(),
            state: ItemState::Open,
            draft: false,
            checks: CheckCounts::default(),
            failing: vec![],
            mergeable: None,
            blocked: false,
            queued: None,
            merged_by: None,
            merged_at_ms: None,
            met: true,
            error: Some(format!("no pull request {pr} that you can see")),
            counted: 0,
            pending: 0,
        }
    }

    /// The pull request's verdict from what GitHub said of it. `queue`:
    /// `(its branch has a merge queue, it's in it)`.
    pub fn of(pr: &PrRef, item: &Item, checks: &[Check], queue: Option<(bool, bool)>) -> PrVerdict {
        let mut counts = CheckCounts::default();
        let mut failing = Vec::new();
        let counted: Vec<&Check> = checks
            .iter()
            .filter(|c| {
                !c.allow_failure && !matches!(c.state, CheckState::Skipped | CheckState::Neutral | CheckState::Manual)
            })
            .collect();
        let pending = counted.iter().filter(|c| matches!(c.state, CheckState::Queued | CheckState::Running)).count();
        for c in checks {
            match c.state {
                CheckState::Success => counts.success += 1,
                CheckState::Failure => {
                    counts.failure += 1;
                    if !c.allow_failure {
                        failing.push(c.name.clone());
                    }
                }
                CheckState::Running => counts.running += 1,
                CheckState::Queued => counts.queued += 1,
                CheckState::Skipped => counts.skipped += 1,
                CheckState::ActionRequired | CheckState::Cancelled => {
                    counts.other += 1;
                    if !c.allow_failure {
                        failing.push(c.name.clone());
                    }
                }
                CheckState::Neutral | CheckState::Manual => counts.other += 1,
            }
        }
        PrVerdict {
            pr: pr.to_string(),
            url: item.url.clone(),
            state: item.state,
            draft: item.draft,
            checks: counts,
            failing,
            mergeable: item.mergeable,
            blocked: item.blocked,
            queued: queue.filter(|(enabled, _)| *enabled).map(|(_, in_queue)| in_queue),
            merged_by: item.merged_by.clone(),
            merged_at_ms: item.merged_at,
            met: false,
            error: None,
            counted: counted.len() as u32,
            pending: pending as u32,
        }
    }

    /// Whether this is what `want` waits for. `settled`: the wait has gone
    /// on long enough that a PR with no checks has none to wait for.
    /// A pull request that's no longer open can't become what a check or a
    /// merge wait is for, so it ends those too, saying what became of it.
    pub fn meets(&self, want: PrWant, settled: bool) -> bool {
        if self.error.is_some() {
            return true;
        }
        let open = self.state == ItemState::Open;
        let none_yet = self.counted == 0 && !settled;
        match want {
            PrWant::Merged => self.state == ItemState::Merged,
            PrWant::Closed => !open,
            _ if !open => true,
            PrWant::ChecksFailed => !self.failing.is_empty(),
            PrWant::ChecksDone => self.pending == 0 && !none_yet,
            PrWant::Ready => {
                !self.draft
                    && self.failing.is_empty()
                    && self.pending == 0
                    && !none_yet
                    && self.mergeable == Some(true)
                    && !self.blocked
            }
        }
    }

    /// One line for the answer's summary.
    pub fn line(&self) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        let c = &self.checks;
        let mut s =
            format!("{}: {}", self.pr, serde_json::to_value(self.state).unwrap_or_default().as_str().unwrap_or(""));
        if self.draft {
            s += ", draft";
        }
        s += &format!(", checks {} passed, {} failed, {} running", c.success, c.failure, c.running + c.queued);
        if !self.failing.is_empty() {
            s += &format!(" (failing: {})", self.failing.join(", "));
        }
        s += match self.mergeable {
            Some(true) => ", mergeable",
            Some(false) => ", not mergeable (conflicts)",
            None => ", mergeable not known yet",
        };
        if self.blocked {
            s += ", blocked by branch protection";
        }
        match self.queued {
            Some(true) => s += ", in the merge queue",
            Some(false) => s += ", not in the merge queue",
            None => {}
        }
        if let Some(by) = &self.merged_by {
            s += &format!(", merged by {by}");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_proto::forge::CheckSource;

    fn pr() -> PrRef {
        PrRef::parse("acme/app#7").unwrap()
    }

    fn check(name: &str, state: CheckState) -> Check {
        Check {
            name: name.into(),
            source: CheckSource::CheckRun,
            state,
            allow_failure: false,
            url: None,
            description: None,
            run: None,
        }
    }

    fn verdict(checks: &[Check], mergeable: Option<bool>, state: ItemState) -> PrVerdict {
        let item = Item { state, mergeable, ..Default::default() };
        PrVerdict::of(&pr(), &item, checks, None)
    }

    #[test]
    fn a_reference_is_owner_repo_number_or_a_link() {
        let p = PrRef::parse(" acme/app#7 ").unwrap();
        assert_eq!((p.host.as_str(), p.repo.as_str(), p.number), ("github.com", "acme/app", 7));
        let p = PrRef::parse("https://ghe.example/acme/app/pull/12/files?x=1").unwrap();
        assert_eq!((p.host.as_str(), p.repo.as_str(), p.number), ("ghe.example", "acme/app", 12));
        for bad in
            ["7", "%7", "acme/app", "acme/app#0", "acme/app#x", "a/b/c#1", "https://github.com/acme/app/issues/1"]
        {
            assert!(PrRef::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn checks_are_counted_by_state_and_the_failing_named() {
        let v = verdict(
            &[
                check("build", CheckState::Success),
                check("lint", CheckState::Failure),
                check("e2e", CheckState::Running),
                check("deploy", CheckState::Queued),
                check("docs", CheckState::Skipped),
            ],
            Some(true),
            ItemState::Open,
        );
        assert_eq!(
            (v.checks.success, v.checks.failure, v.checks.running, v.checks.queued, v.checks.skipped),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(v.failing, ["lint"]);
        assert!(v.meets(PrWant::ChecksFailed, false), "a failure doesn't wait for the rest");
        assert!(!v.meets(PrWant::ChecksDone, false));
        assert!(!v.meets(PrWant::Ready, false));
    }

    #[test]
    fn ready_is_green_and_mergeable_and_not_held() {
        let green = [check("build", CheckState::Success), check("docs", CheckState::Skipped)];
        assert!(verdict(&green, Some(true), ItemState::Open).meets(PrWant::Ready, false));
        assert!(!verdict(&green, None, ItemState::Open).meets(PrWant::Ready, false), "unknown isn't mergeable");
        assert!(!verdict(&green, Some(false), ItemState::Open).meets(PrWant::Ready, false));
        let mut held = verdict(&green, Some(true), ItemState::Open);
        held.blocked = true;
        assert!(!held.meets(PrWant::Ready, false));
        assert!(held.meets(PrWant::ChecksDone, false));
        let mut draft = verdict(&green, Some(true), ItemState::Open);
        draft.draft = true;
        assert!(!draft.meets(PrWant::Ready, false));
    }

    #[test]
    fn no_checks_yet_is_waited_on_until_settled() {
        let v = verdict(&[], Some(true), ItemState::Open);
        assert!(!v.meets(PrWant::ChecksDone, false));
        assert!(v.meets(PrWant::ChecksDone, true));
        assert!(v.meets(PrWant::Ready, true));
    }

    #[test]
    fn merged_and_closed_end_what_cant_come_true() {
        let red = [check("build", CheckState::Running)];
        let merged = verdict(&red, Some(true), ItemState::Merged);
        let closed = verdict(&red, Some(true), ItemState::Closed);
        assert!(merged.meets(PrWant::Merged, false));
        assert!(!closed.meets(PrWant::Merged, false), "closed unmerged isn't merged");
        assert!(merged.meets(PrWant::Closed, false) && closed.meets(PrWant::Closed, false));
        assert!(closed.meets(PrWant::Ready, false) && closed.meets(PrWant::ChecksDone, false));
        assert!(!verdict(&red, Some(true), ItemState::Open).meets(PrWant::Merged, false));
    }

    #[test]
    fn the_merge_queue_is_said_only_where_the_branch_has_one() {
        let item = Item::default();
        assert_eq!(PrVerdict::of(&pr(), &item, &[], Some((false, false))).queued, None);
        assert_eq!(PrVerdict::of(&pr(), &item, &[], Some((true, false))).queued, Some(false));
        assert_eq!(PrVerdict::of(&pr(), &item, &[], Some((true, true))).queued, Some(true));
    }

    #[test]
    fn a_pr_that_cant_be_read_ends_the_wait_with_one_answer() {
        let v = PrVerdict::gone(&pr());
        assert!(v.meets(PrWant::Merged, false));
        assert_eq!(v.error.as_deref(), Some("no pull request acme/app#7 that you can see"));
    }
}
