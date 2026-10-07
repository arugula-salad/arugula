//! The normalized model of a pull request (S23's structs): an item, its
//! reviews, checks and timeline events, the same whichever forge it's on.
//! An adapter per provider ([`super::forgejo`], [`super::gitlab`]) maps a forge's answers onto
//! it. The attention rules are written against it, not against a forge.

use serde::{Deserialize, Serialize};

// The model types the block's state is made of are proto's (`arugula_proto::forge`),
// which the web client's types are made from; this module keeps their logic.
pub use arugula_proto::forge::{
    Branch, Check, CheckSource, CheckState, Event, EventKind, Issue, Item, ItemKind, ItemState, Linked, Pr, Provider,
    Review, ReviewState, Reviewer, RunRef,
};

/// The checks' sum: failure if any counted check is red, else running if
/// any isn't done, else success; `None` when nothing counts. Skipped,
/// neutral and manual checks, and ones allowed to fail, don't count.
pub fn rollup(checks: &[Check]) -> Option<CheckState> {
    let counted: Vec<&Check> = checks
        .iter()
        .filter(|c| {
            !c.allow_failure && !matches!(c.state, CheckState::Skipped | CheckState::Neutral | CheckState::Manual)
        })
        .collect();
    if counted.is_empty() {
        return None;
    }
    if counted.iter().any(|c| c.state.red()) {
        return Some(CheckState::Failure);
    }
    if counted.iter().any(|c| matches!(c.state, CheckState::Queued | CheckState::Running)) {
        return Some(CheckState::Running);
    }
    Some(CheckState::Success)
}

/// Who "you" are on a forge (`GET /user`, `GET /user/teams`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub login: String,
    /// `Org/Name` and `Name` for each team.
    pub teams: Vec<String>,
}

/// What a PR wants of you, most pressing first (S23's rules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    /// Open, and a review is requested from you or one of your teams.
    Review { why: String },
    /// Your open PR's checks are red: which.
    Failed { why: String, checks: Vec<Check> },
    /// Your open PR has changes requested.
    Changes { why: String },
    /// Someone mentioned you after you last looked.
    Mention { why: String, at: i64, event: String },
    /// Your PR merged, or it's open, green and nothing holds it (M37: an
    /// issue assigned to you closed).
    Done { why: String, key: String },
    /// M37: an open issue was given to you after you last looked.
    Assigned { why: String },
}

impl Want {
    pub fn why(&self) -> &str {
        match self {
            Want::Review { why }
            | Want::Failed { why, .. }
            | Want::Changes { why }
            | Want::Mention { why, .. }
            | Want::Done { why, .. }
            | Want::Assigned { why } => why,
        }
    }
}

/// Every reviewer's latest decisive review (approve, changes, dismissed;
/// comments don't change where they stand).
fn decisive(reviews: &[Review]) -> Vec<&Review> {
    let mut sorted: Vec<&Review> = reviews.iter().collect();
    sorted.sort_by_key(|r| r.at.unwrap_or(0));
    let mut last: Vec<&Review> = Vec::new();
    for r in sorted {
        if !matches!(r.state, ReviewState::Approved | ReviewState::ChangesRequested | ReviewState::Dismissed) {
            continue;
        }
        last.retain(|o| o.author != r.author);
        last.push(r);
    }
    last
}

/// `@login` in a text, as a word.
pub fn mentions(body: &str, login: &str) -> bool {
    if login.is_empty() {
        return false;
    }
    let want = format!("@{}", login.to_lowercase());
    let lower = body.to_lowercase();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
    let mut from = 0;
    while let Some(i) = lower[from..].find(&want) {
        let at = from + i;
        let before = lower[..at].chars().next_back();
        let after = lower[at + want.len()..].chars().next();
        if before.is_none_or(|c| !word(c) || c == '.')
            && after.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
        {
            return true;
        }
        from = at + want.len();
    }
    false
}

/// What `pr` wants of `me`, for mentions newer than `seen` (ms).
pub fn attention(pr: &Pr, me: &Me, seen: i64) -> Vec<Want> {
    let it = &pr.item;
    let mine = !me.login.is_empty() && it.author.eq_ignore_ascii_case(&me.login);
    let open = it.state == ItemState::Open;
    let mut out = Vec::new();
    let req = it.requested.iter().find(|r| match r {
        Reviewer::User(u) => u.eq_ignore_ascii_case(&me.login),
        Reviewer::Team(t) => me.teams.iter().any(|m| m.eq_ignore_ascii_case(t)),
    });
    if open && let Some(r) = req {
        let why = match r {
            Reviewer::User(_) => "review requested from you".to_owned(),
            Reviewer::Team(t) => format!("review requested from {t}"),
        };
        out.push(Want::Review { why });
    }
    if mine && open && pr.rollup == Some(CheckState::Failure) {
        let red: Vec<Check> = pr.checks.iter().filter(|c| c.state.red() && !c.allow_failure).cloned().collect();
        let names: Vec<&str> = red.iter().take(3).map(|c| c.name.as_str()).collect();
        let more = if red.len() > 3 { format!(" (+{})", red.len() - 3) } else { String::new() };
        let why =
            format!("{} check{} failed: {}{more}", red.len(), if red.len() == 1 { "" } else { "s" }, names.join(", "));
        out.push(Want::Failed { why, checks: red });
    }
    let last = decisive(&pr.reviews);
    let changes: Vec<&str> =
        last.iter().filter(|r| r.state == ReviewState::ChangesRequested).filter_map(|r| r.author.as_deref()).collect();
    if mine && open && !changes.is_empty() {
        out.push(Want::Changes { why: format!("changes requested by {}", changes.join(", ")) });
    }
    let mention = pr.events.iter().rfind(|e| {
        e.at > seen
            && e.actor.as_deref().is_none_or(|a| !a.eq_ignore_ascii_case(&me.login))
            && (e.body.as_deref().is_some_and(|b| mentions(b, &me.login))
                || (e.kind == EventKind::Mentioned
                    && matches!(&e.target, Some(Reviewer::User(u)) if u.eq_ignore_ascii_case(&me.login))))
    });
    if let Some(e) = mention {
        let by = e.actor.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
        out.push(Want::Mention { why: format!("mentioned{by}"), at: e.at, event: e.id.clone() });
    }
    if mine && it.state == ItemState::Merged {
        out.push(Want::Done { why: "merged".into(), key: "merged".into() });
    } else if mine
        && open
        && pr.rollup == Some(CheckState::Success)
        && changes.is_empty()
        && !it.draft
        && it.mergeable != Some(false)
        && !it.blocked
    {
        let approved = last.iter().any(|r| r.state == ReviewState::Approved);
        // Forgejo can't say whether branch protection holds it: "checks
        // green", never "ready to merge" alone.
        let why = if approved { "approved, checks green" } else { "checks green" };
        out.push(Want::Done { why: why.into(), key: format!("green:{}", it.head.sha) });
    }
    out
}

fn when(ms: i64) -> String {
    let secs = ms / 1000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn state_word<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(|s| s.replace('_', " "))).unwrap_or_default()
}

/// An event as a line of text.
pub trait EventLine {
    /// One line: when, who, what (and a body's first line).
    fn line(&self) -> String;
}

impl EventLine for Event {
    fn line(&self) -> String {
        let who = self.actor.as_deref().unwrap_or("someone");
        let what = match self.kind {
            EventKind::Commented => "commented".to_owned(),
            EventKind::ReviewComment => "commented on the code".to_owned(),
            EventKind::Reviewed => "reviewed".to_owned(),
            EventKind::ReviewRequested => {
                format!("asked {} for a review", self.target.as_ref().map_or("someone", Reviewer::name))
            }
            EventKind::ReviewRequestRemoved => "took back a review request".to_owned(),
            EventKind::ReviewDismissed => "dismissed a review".to_owned(),
            EventKind::Mentioned => {
                format!("mentioned {}", self.target.as_ref().map_or("someone", Reviewer::name))
            }
            EventKind::Pushed => match self.commits {
                Some(n) => format!(
                    "pushed {n} commit{}{}",
                    if n == 1 { "" } else { "s" },
                    if self.force { " (forced)" } else { "" }
                ),
                None => "pushed".to_owned(),
            },
            EventKind::Other => self.what.clone().unwrap_or_else(|| "did something".into()),
            // M37: who was given (or let go of) an issue, and what
            // referred to it.
            EventKind::Assigned => match (&self.target, self.what.as_deref()) {
                (Some(t), Some("unassigned")) => format!("unassigned {}", t.name()),
                (Some(t), _) => format!("assigned {}", t.name()),
                _ => "assigned".into(),
            },
            EventKind::Referenced => match self.what.as_deref() {
                Some(w) if w.starts_with('#') => format!("referenced it in {w}"),
                _ => "referenced it".into(),
            },
            k => state_word(&k),
        };
        let body = self
            .body
            .as_deref()
            .and_then(|b| b.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(|l| format!(": {}", clip(l, 160)))
            .unwrap_or_default();
        format!("{} {who} {what}{body}", when(self.at))
    }
}

/// What an issue wants of `me` (M37), for events newer than `seen` (ms):
/// given to you after you last looked, or a mention, while it's open; and
/// once, when one that's yours closes. A closed issue asks nothing else.
pub fn issue_attention(issue: &Issue, me: &Me, seen: i64) -> Vec<Want> {
    let it = &issue.item;
    let mut out = Vec::new();
    if me.login.is_empty() {
        return out;
    }
    let mine = it.assignees.iter().any(|a| a.eq_ignore_ascii_case(&me.login));
    if it.state != ItemState::Open {
        if mine {
            out.push(Want::Done { why: "closed".into(), key: "closed".into() });
        }
        return out;
    }
    if mine {
        // The newest event that gave it to you; someone else's doing.
        let given = issue.events.iter().rfind(|e| {
            e.kind == EventKind::Assigned
                && e.what.as_deref() != Some("unassigned")
                && e.target.as_ref().is_some_and(|t| t.name().eq_ignore_ascii_case(&me.login))
        });
        let news = match given {
            Some(e) => e.at > seen && e.actor.as_deref().is_none_or(|a| !a.eq_ignore_ascii_case(&me.login)),
            // Assigned before the timeline we kept: news until looked at.
            None => seen == 0,
        };
        if news {
            let by = given.and_then(|e| e.actor.as_deref()).map(|a| format!(" by {a}")).unwrap_or_default();
            out.push(Want::Assigned { why: format!("assigned to you{by}") });
        }
    }
    let mention = issue.events.iter().rfind(|e| {
        e.at > seen
            && e.actor.as_deref().is_none_or(|a| !a.eq_ignore_ascii_case(&me.login))
            && e.body.as_deref().is_some_and(|b| mentions(b, &me.login))
    });
    // The issue's own text counts too, while you've never looked.
    let in_body = seen == 0 && !it.author.eq_ignore_ascii_case(&me.login) && mentions(&it.body, &me.login);
    if let Some(e) = mention {
        let by = e.actor.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
        out.push(Want::Mention { why: format!("mentioned{by}"), at: e.at, event: e.id.clone() });
    } else if in_body {
        out.push(Want::Mention { why: format!("mentioned by {}", it.author), at: it.updated_at, event: "body".into() });
    }
    out
}

/// A pull request or an issue as text, for `capture --text` and agents.
pub trait ItemText {
    fn text(&self, repo: &str) -> String;
}

impl ItemText for Issue {
    /// The issue as text: header, body, the pull requests that refer to
    /// it, the timeline.
    fn text(&self, repo: &str) -> String {
        let it = &self.item;
        let mut out = vec![format!("{repo}#{} {}", it.number, it.title)];
        out.push(format!("issue · {} · opened by {} · {}", state_word(&it.state), it.author, it.url));
        if !it.labels.is_empty() {
            out.push(format!("labels: {}", it.labels.join(", ")));
        }
        out.push(if it.assignees.is_empty() {
            "assigned to nobody".into()
        } else {
            format!("assigned to: {}", it.assignees.join(", "))
        });
        if !it.body.trim().is_empty() {
            out.push(String::new());
            out.push(it.body.trim().to_owned());
        }
        out.push(String::new());
        out.push(format!("pull requests: {}", self.linked.len()));
        for l in &self.linked {
            let head = l.head.as_deref().map(|h| format!(" ({h})")).unwrap_or_default();
            out.push(format!("  #{} {} {}{head}", l.number, state_word(&l.state), l.title));
        }
        out.push("timeline:".into());
        for e in &self.events {
            out.push(format!("  {}", e.line()));
        }
        out.join("\n") + "\n"
    }
}

/// A branch name for working on an issue: `i89-short-kebab-title`.
pub fn branch_for(number: u64, title: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let title = title.to_lowercase().replace(['\'', '’'], "");
    for w in title.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let len: usize = words.iter().map(|w| w.len() + 1).sum();
        if words.len() >= 5 || (len + w.len() > 28 && !words.is_empty()) {
            break;
        }
        words.push(w.chars().take(28).collect());
    }
    if words.is_empty() { format!("i{number}") } else { format!("i{number}-{}", words.join("-")) }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

impl ItemText for Pr {
    /// The PR as text: header, body, reviews, checks, the timeline.
    fn text(&self, repo: &str) -> String {
        let it = &self.item;
        let mut out = vec![format!("{repo}#{} {}", it.number, it.title)];
        let state = if it.draft && it.state == ItemState::Open { "draft".into() } else { state_word(&it.state) };
        out.push(format!(
            "{state} · {} wants to merge {} into {} · {}",
            it.author,
            it.head
                .repo
                .as_deref()
                .filter(|r| *r != repo)
                .map_or(it.head.branch.clone(), |r| format!("{r}:{}", it.head.branch)),
            it.base.branch,
            it.url
        ));
        if !it.labels.is_empty() {
            out.push(format!("labels: {}", it.labels.join(", ")));
        }
        if !it.requested.is_empty() {
            out.push(format!(
                "review requested from: {}",
                it.requested.iter().map(Reviewer::name).collect::<Vec<_>>().join(", ")
            ));
        }
        if let Some(m) = it.mergeable.filter(|_| it.state == ItemState::Open) {
            out.push(if m { "mergeable".into() } else { "has conflicts: not mergeable".into() });
        }
        if !it.body.trim().is_empty() {
            out.push(String::new());
            out.push(it.body.trim().to_owned());
        }
        out.push(String::new());
        out.push(match self.rollup {
            Some(r) => format!("checks: {} ({})", state_word(&r), self.checks.len()),
            None => "checks: none".into(),
        });
        for c in &self.checks {
            let d = c.description.as_deref().map(|d| format!(" — {d}")).unwrap_or_default();
            out.push(format!("  {:<9} {}{d}", state_word(&c.state), c.name));
        }
        out.push(format!("reviews: {}", self.reviews.len()));
        for r in &self.reviews {
            let body = r
                .body
                .as_deref()
                .filter(|b| !b.trim().is_empty())
                .map(|b| format!(": {}", clip(b.trim(), 200)))
                .unwrap_or_default();
            let stale = if r.stale { " (stale)" } else { "" };
            out.push(format!("  {} {}{stale}{body}", r.author.as_deref().unwrap_or("?"), state_word(&r.state)));
        }
        out.push("timeline:".into());
        for e in &self.events {
            out.push(format!("  {}", e.line()));
        }
        out.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(state: CheckState) -> Check {
        Check {
            name: format!("{state:?}"),
            source: CheckSource::Status,
            state,
            allow_failure: false,
            url: None,
            description: None,
            run: None,
        }
    }

    #[test]
    fn rollups() {
        use CheckState::*;
        assert_eq!(rollup(&[]), None);
        assert_eq!(rollup(&[check(Skipped), check(Neutral)]), None);
        assert_eq!(rollup(&[check(Success), check(Skipped)]), Some(Success));
        assert_eq!(rollup(&[check(Success), check(Running)]), Some(Running));
        assert_eq!(rollup(&[check(Queued)]), Some(Running));
        assert_eq!(rollup(&[check(Running), check(Failure)]), Some(Failure));
        assert_eq!(rollup(&[check(Cancelled)]), Some(Failure));
        assert_eq!(rollup(&[check(ActionRequired)]), Some(Failure));
        let allowed = Check { allow_failure: true, ..check(Failure) };
        assert_eq!(rollup(&[allowed, check(Success)]), Some(Success));
    }

    #[test]
    fn mentions_are_words() {
        assert!(mentions("Thanks @wetneb!", "wetneb"));
        assert!(mentions("@WetNeb look", "wetneb"));
        assert!(mentions("cc @jake.", "jake"));
        assert!(!mentions("@wetnebs", "wetneb"));
        assert!(!mentions("me@wetneb.org", "wetneb"));
        assert!(!mentions("@wetneb-bot", "wetneb"));
        assert!(!mentions("anything", ""));
    }

    #[test]
    fn branches_for_issues() {
        assert_eq!(
            branch_for(89, "M37: issue blocks, and issue → agent (worktree, agent, PR in a tab)"),
            "i89-m37-issue-blocks-and-issue"
        );
        assert_eq!(branch_for(7, "Fix: the `tea` login's ssh_host!"), "i7-fix-the-tea-logins-ssh");
        assert_eq!(branch_for(3, "→ ✓"), "i3");
        assert_eq!(
            branch_for(4, "Supercalifragilisticexpialidociousness everywhere"),
            "i4-supercalifragilisticexpialid"
        );
    }

    #[test]
    fn times() {
        assert_eq!(when(0), "1970-01-01 00:00");
        assert_eq!(when(1_790_975_391_548), "2026-10-02 21:09");
    }
}
