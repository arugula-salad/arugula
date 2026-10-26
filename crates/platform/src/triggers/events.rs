//! Event rules (M214): a linked repo's GitHub event, as the App's webhook
//! brings it ([`crate::forge::webhook`]), starts the specs whose rules it
//! matches. Three events, each read from the payload, never its text:
//!
//! | Rule | Webhook | Its branch |
//! |---|---|---|
//! | `pr_opened` | `pull_request`, `opened` | the PR's base |
//! | `check_failed` | `check_run` completed `failure` or `timed_out`; `status` `failure` or `error` | the check suite's head branch and its PRs' bases; the status's branches |
//! | `push` | `push` to a branch (not a tag, not a delete) | the branch |
//!
//! Two more go to an agent session, never a rule (M260,
//! [`crate::sessions::feedback`]): a `pull_request_review` submitted and a
//! `pull_request_review_comment` made, on a PR from the repo's own branch
//! (`RuleEvent::Review`). Both count from insiders only, by the
//! review's or comment's `author_association`, as a PR's does.
//!
//! **A session's first.** Before any rule, an event on a session's branch
//! (a check that failed on its head, a review on its PR) goes back to that
//! session ([`crate::sessions::feedback::route`]), and to no rule: rules
//! see the rest as before.
//!
//! A rule fires for a repo linked to its project (and the repo it names,
//! if it names one), on a branch it names (`*` any run of characters) or
//! any. Each match makes one run, keyed by the rule and the event's own
//! identity (the PR, the check run or status, the push's ref and commit),
//! so a redelivery, or the same event seen twice, makes no second run.
//!
//! **From outside.** Anyone can open a PR on a public repo, from a fork,
//! and its checks run. So by default a rule refuses an event from outside
//! the repo ([`Seen::outside`]): a PR from a fork or by someone GitHub
//! doesn't call its `OWNER`, `MEMBER` or `COLLABORATOR`, and a check run
//! whose head isn't in the repo (a fork's, or one that doesn't say). A
//! fork's branch names are its author's, so they never match a rule's
//! branch. A push and a commit status need write access to the repo, so
//! they're from inside. A rule takes outsiders' events only when it says so
//! (`outsiders`), and every rule starts at most `max_per_hour` runs an hour.
//!
//! **Untrusted text.** What an event's source says (branches, a check's
//! name, its author's standing) is the payload's, written by whoever
//! caused it. It's kept to show, quoted, and never reaches a run's
//! environment, command or an agent's prompt unquoted.

use serde_json::{Value, json};

use super::Gate;
use crate::{
    Platform,
    db::{
        Db,
        runs::TriggerKind,
        triggers::{Rule, RuleEvent, Start, Started},
    },
};

/// An event a rule can match, as read from a webhook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub event: RuleEvent,
    /// GitHub's id for the repo.
    pub repo_id: u64,
    /// `owner/name`.
    pub repo: String,
    /// The branches it's on (a status's commit may be on several). Never a
    /// fork's.
    pub branches: Vec<String>,
    /// The repo's own branches whose head it's about: a check's head
    /// branch and its PRs' heads, a status's one branch, a reviewed PR's
    /// head. Never a fork's. What finds a session's (M260).
    pub heads: Vec<String>,
    /// From outside the repo: a fork, or someone who isn't its owner,
    /// member or collaborator, or a check run that doesn't say its head is
    /// in the repo.
    pub outside: bool,
    /// What it is, in the repo: a run's key names it.
    pub key: String,
    /// What a run's trigger source says of it: numbers, a sha, a check's
    /// name, its author's standing and whether it's a fork's. Untrusted.
    pub detail: Value,
}

/// GitHub's word for someone who may write to the repo.
fn insider(association: Option<&str>) -> bool {
    matches!(association, Some("OWNER" | "MEMBER" | "COLLABORATOR"))
}

/// The event a webhook brings that a rule can match, if it brings one.
pub fn seen(event: &str, v: &Value) -> Option<Seen> {
    let repo_id = v["repository"]["id"].as_u64()?;
    let repo = v["repository"]["full_name"].as_str().filter(|r| r.contains('/'))?.to_owned();
    let s = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let ours = |r: &Value| r["id"].as_u64() == Some(repo_id);
    let (rule, branches, heads, outside, key, detail) = match event {
        "pull_request" if v["action"] == "opened" => {
            let pr = &v["pull_request"];
            let number = pr["number"].as_u64()?;
            let association = s(&pr["author_association"]);
            let fork = !ours(&pr["head"]["repo"]);
            let detail = json!({
                "number": number,
                "sha": s(&pr["head"]["sha"]),
                "base": s(&pr["base"]["ref"]),
                "head_repo": s(&pr["head"]["repo"]["full_name"]),
                "fork": fork,
                "author_association": association,
            });
            let outside = fork || !insider(association.as_deref());
            let branches = s(&pr["base"]["ref"]).into_iter().collect();
            (RuleEvent::PrOpened, branches, vec![], outside, format!("pr:{repo_id}:{number}"), detail)
        }
        "check_run"
            if v["action"] == "completed"
                && matches!(v["check_run"]["conclusion"].as_str(), Some("failure" | "timed_out")) =>
        {
            let run = &v["check_run"];
            let id = run["id"].as_u64()?;
            let suite = &run["check_suite"];
            // GitHub lists only a PR whose head is in the repo here: a
            // fork's check run names none.
            let prs = run["pull_requests"].as_array().cloned().unwrap_or_default();
            let ours_by_pr = !prs.is_empty() && prs.iter().all(|p| ours(&p["head"]["repo"]));
            let inside = ours(&suite["head_repository"]) || ours_by_pr;
            let mut branches: Vec<String> = prs.iter().filter_map(|p| s(&p["base"]["ref"])).collect();
            let mut heads: Vec<String> = vec![];
            if inside {
                branches.extend(s(&suite["head_branch"]));
                heads.extend(s(&suite["head_branch"]));
            }
            for p in prs.iter().filter(|p| ours(&p["head"]["repo"])) {
                heads.extend(s(&p["head"]["ref"]).filter(|h| !heads.contains(h)));
            }
            let detail = json!({
                "check": s(&run["name"]),
                "sha": s(&run["head_sha"]),
                "conclusion": s(&run["conclusion"]),
                "head_repo": s(&suite["head_repository"]["full_name"]),
                "fork": !inside,
            });
            (RuleEvent::CheckFailed, branches, heads, !inside, format!("check_run:{repo_id}:{id}"), detail)
        }
        // Only someone who may write to the repo, or an app on it, posts a
        // status, and its branches are the repo's that hold the commit.
        "status" if matches!(v["state"].as_str(), Some("failure" | "error")) => {
            let id = v["id"].as_u64()?;
            let detail = json!({ "check": s(&v["context"]), "sha": s(&v["sha"]), "conclusion": s(&v["state"]) });
            let branches: Vec<String> =
                v["branches"].as_array().into_iter().flatten().filter_map(|b| s(&b["name"])).collect();
            // A commit on several branches (merged on) is no one session's.
            let heads = if branches.len() == 1 { branches.clone() } else { vec![] };
            (RuleEvent::CheckFailed, branches, heads, false, format!("status:{repo_id}:{id}"), detail)
        }
        // A push needs write access to the repo.
        "push" if v["deleted"] != true => {
            let branch = v["ref"].as_str()?.strip_prefix("refs/heads/")?.to_owned();
            let sha = s(&v["after"])?;
            let key = format!("push:{repo_id}:{branch}:{sha}");
            let detail = json!({ "branch": branch, "sha": sha, "pusher": s(&v["pusher"]["name"]) });
            (RuleEvent::Push, vec![branch], vec![], false, key, detail)
        }
        // A review (M260): submitted, never a reviewer's own draft.
        "pull_request_review"
            if v["action"] == "submitted"
                && v["review"]["state"].as_str().is_some_and(|st| !st.eq_ignore_ascii_case("pending")) =>
        {
            let (pr, review) = (&v["pull_request"], &v["review"]);
            let id = review["id"].as_u64()?;
            let association = s(&review["author_association"]);
            let fork = !ours(&pr["head"]["repo"]);
            let detail = json!({
                "number": pr["number"].as_u64(),
                "review": id,
                "state": s(&review["state"]).map(|st| st.to_ascii_lowercase()),
                "sha": s(&review["commit_id"]),
                "author_association": association,
                "fork": fork,
            });
            let outside = fork || !insider(association.as_deref());
            let heads = if fork { vec![] } else { s(&pr["head"]["ref"]).into_iter().collect() };
            (RuleEvent::Review, vec![], heads, outside, format!("review:{repo_id}:{id}"), detail)
        }
        // One of a review's inline comments (M260).
        "pull_request_review_comment" if v["action"] == "created" => {
            let (pr, comment) = (&v["pull_request"], &v["comment"]);
            let id = comment["id"].as_u64()?;
            let association = s(&comment["author_association"]);
            let fork = !ours(&pr["head"]["repo"]);
            let detail = json!({
                "number": pr["number"].as_u64(),
                "review": comment["pull_request_review_id"].as_u64(),
                "comment": id,
                "sha": s(&comment["commit_id"]),
                "author_association": association,
                "fork": fork,
            });
            let outside = fork || !insider(association.as_deref());
            let heads = if fork { vec![] } else { s(&pr["head"]["ref"]).into_iter().collect() };
            (RuleEvent::Review, vec![], heads, outside, format!("review_comment:{repo_id}:{id}"), detail)
        }
        _ => return None,
    };
    Some(Seen { event: rule, repo_id, repo, branches, heads, outside, key, detail })
}

/// Whether `name` fits `pattern`, where `*` is any run of characters.
pub fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => {
            let Some(name) = name.strip_prefix(head) else { return false };
            (0..=name.len()).filter(|i| name.is_char_boundary(*i)).any(|i| glob(rest, &name[i..]))
        }
    }
}

/// Whether `rule` fires for `seen`: the event and repo are the caller's
/// ([`Db::rules_for`]); its branch, if it names one, is one of the event's.
/// Whether it takes an event from outside is [`fire`]'s to say.
pub fn matches(rule: &Rule, seen: &Seen) -> bool {
    match &rule.branch {
        None => true,
        Some(b) => seen.branches.iter().any(|name| glob(b, name)),
    }
}

/// Start each spec whose rule `seen` matches, while the rule's maker may
/// (`gate`), within the rule's hourly cap: what came of each. One rule's
/// failure doesn't stop the rest; the caller sees it beside them.
pub async fn fire(
    db: &Db,
    gate: &dyn Gate,
    seen: &Seen,
    delivery: &str,
    now: u64,
) -> anyhow::Result<Vec<(Rule, anyhow::Result<Started>)>> {
    let mut out = vec![];
    if seen.event == RuleEvent::Review {
        return Ok(out);
    }
    for (rule, spec) in db.rules_for(seen.repo_id, seen.event)? {
        if !matches(&rule, seen) {
            continue;
        }
        if seen.outside && !rule.outsiders {
            out.push((rule, Ok(Started::Outsider)));
            continue;
        }
        let started = async {
            if let Some(why) = gate.refuses(&spec.project, rule.created_by.as_deref()).await? {
                return Ok(Started::Refused(why));
            }
            let key = format!("rule:{}:{}", rule.id, seen.key);
            let source = json!({
                "rule": rule.id,
                "spec": spec.id,
                "event": seen.event.as_str(),
                "repo": seen.repo,
                "delivery": delivery,
                "detail": seen.detail,
            });
            let start = Start {
                project: &spec.project,
                workspace: &spec.workspace,
                op: &spec.op,
                trigger: TriggerKind::Event,
                source: &source,
                started_by: &rule.id,
                started_by_agent: None,
                key: Some(&key),
            };
            db.fire_rule(&rule, &start, now)
        }
        .await;
        out.push((rule, started));
    }
    Ok(out)
}

/// The webhook's hook: what goes back to a session (M260), else the runs a
/// linked repo's event starts, after its mirror rows are written. `false`
/// when a rule (or reading them, or a session's feedback) failed: the
/// webhook forgets the delivery and answers 500, so GitHub's redelivery
/// tries again (each run's key, and each feedback item's, keeps it from
/// happening twice).
pub async fn webhook(p: &std::sync::Arc<Platform>, event: &str, delivery: &str, v: &Value) -> bool {
    let now = arugula_e2e::now_ms();
    if let Err(e) = crate::sessions::feedback::suite_done(p, event, v, now) {
        tracing::warn!(event, delivery, error = format!("{e:#}"), "a check suite on a session's branch");
        return false;
    }
    let Some(seen) = seen(event, v) else { return true };
    match crate::sessions::feedback::route(p, &seen, v, now) {
        Ok(crate::sessions::feedback::Routed::Elsewhere) => {}
        Ok(routed) => {
            tracing::info!(event, delivery, ?routed, "a webhook's event went to a session");
            return true;
        }
        Err(e) => {
            tracing::warn!(event, delivery, error = format!("{e:#}"), "sending a webhook's event to its session");
            return false;
        }
    }
    let gate = super::Members(p.clone());
    let fired = match fire(&p.db, &gate, &seen, delivery, now).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(event, delivery, error = format!("{e:#}"), "reading a webhook's event rules");
            return false;
        }
    };
    let mut ok = true;
    for (rule, started) in fired {
        match started {
            Ok(Started::Made(run)) => {
                tracing::info!(rule = %rule.id, event, delivery, run = %run.id, "an event rule started a run")
            }
            Ok(Started::Already(_)) => {}
            Ok(other) => tracing::warn!(
                rule = %rule.id,
                event,
                delivery,
                why = other.why_not().unwrap_or_default(),
                "an event rule matched and started no run"
            ),
            Err(e) => {
                ok = false;
                tracing::warn!(rule = %rule.id, event, delivery, error = format!("{e:#}"), "starting an event rule's run");
            }
        }
    }
    ok
}
