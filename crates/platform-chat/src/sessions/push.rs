//! A session's push (M259, S42 q2): the token its box pushes the session's
//! branch with, minted for that one push, through the hub's `POST
//! /api/hosted/work/session/push` (`WORK_SESSION_PUSH`), signed as the
//! hosted machine.
//!
//! Installation tokens can't be confined to a ref, so the box confines
//! the push itself (it pushes one refspec from a repo of its own, the
//! daemon's `work_sessions::push_turn`), and the platform mints only for
//! a box asking for exactly what it may push:
//! - the session is on the machine's workspace, and the machine is the one
//!   its first report bound it to, and it hasn't ended;
//! - the ref is the session's own branch, exactly (`arugula/<8 hex>-<slug>`):
//!   never `main`, never another session's;
//! - its repo is linked to the session's project;
//! - it isn't a review (M264), which pushes nothing;
//! - everyone who can steer the session may push to that repo on GitHub
//!   right now ([`steerers`]): its asker; every owner and editor of its
//!   project, who reply to it in its thread (each reply a prompt), change
//!   the workspace's setup and finish steps, and use its box as its owners
//!   do; and every GitHub reviewer whose review was sent to it as a prompt.
//!   For each, the hub's GitHub identity for them (a reviewer's is GitHub's
//!   own), and GitHub's answer about their role on the repo (write or
//!   better: [`GithubApp::may_push`]), asked for each push with a read
//!   token. A session pushes only what each of them could have pushed
//!   themselves. No GitHub identity, a lesser role, an asker whose account
//!   is gone, or GitHub not answering: refused, naming who and why, for
//!   the box.
//!
//! The token covers that repo alone, with `contents: write` and nothing
//! else: not `workflows`. GitHub refuses a push that creates or changes a
//! file under `.github/workflows/` without it, so a session's branch never
//! changes CI; the box says so plainly when that's why a push didn't go
//! (`arugulad-work`'s `sessions_push`). That includes a branch that began
//! before main changed its workflows, since GitHub compares a new
//! branch's tree with the default branch's: a new session starts from
//! main as it is. It's logged as every mint is (`session:push`, for the
//! asker), never kept, and the box revokes it once it pushed. The App's
//! key never leaves the platform.
//!
//! A clone (`clone` in the ask, Fleet 6): a session whose repo is linked
//! but no box's setup cloned clones it at its start, shallow, with a token
//! minted the same way, after the machine, session, branch and link
//! checks (not the asker's GitHub access: it reads what the project
//! already linked), with `contents: read` alone (`session:clone`), and
//! revoked once it's cloned.
//!
//! What this doesn't stop: an agent that gets from its session user to the
//! daemon's user could use a token in the seconds it lives, on any ref.
//! Only control holding the token (a git proxy that reads the ref list)
//! would close that: S42's later hardening.

use arugula_control_wire::work::{PushAsk, PushGiven};
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        session_feedback::{FeedbackKind, FeedbackState},
        sessions::State,
    },
    forge::{Caller, GithubApp, GithubUser, Repo},
    work::HostedMachine,
};

/// Why someone other than the asker counts, after their refusal.
pub const STEERERS: &str = "A session pushes only when everyone who can steer it may push there: its asker, the \
                            project's owners and editors, and GitHub reviewers whose reviews it was sent";

fn refused(m: &HostedMachine, a: &PushAsk, why: &str) -> PushGiven {
    warn!(session = a.session, machine = m.machine, why, "refused a hosted machine's push token");
    PushGiven::Refused { why: why.into() }
}

/// Whether `account` may push to the linked repo on GitHub right now
/// (module docs): their GitHub identity from the hub, then GitHub's
/// answer about their role, through the link's installation. `who` names
/// them in the reason ("the asker", "Bo (editor)"). `Err` is why not, for
/// the box. Fails closed: no identity, no answer, no token.
async fn account_pushes(
    app: &Platform,
    gh: &GithubApp,
    account: &str,
    who: &str,
    link: &crate::db::forge::LinkedRepo,
) -> Result<(), String> {
    let repo = link.full_name.as_str();
    let user = match app.hub.github_user(account).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return Err(format!("{who} hasn't signed in to control with GitHub, so the session can't push to {repo}"));
        }
        Err(e) => {
            warn!(account, error = format!("{e:#}"), "asking the hub for a GitHub identity before a session's push");
            return Err(format!("couldn't check {who}'s GitHub identity, so the session can't push to {repo} now"));
        }
    };
    github_pushes(app, gh, account, &user, who, link).await
}

/// Whether GitHub's `user` may push to the linked repo now, asked for
/// `caller` (logged as theirs).
async fn github_pushes(
    app: &Platform,
    gh: &GithubApp,
    caller: &str,
    user: &GithubUser,
    who: &str,
    link: &crate::db::forge::LinkedRepo,
) -> Result<(), String> {
    let repo = link.full_name.as_str();
    match gh.may_push(&Caller::of(app, caller), link.installation_id, link.repo_id, user, repo).await {
        Ok(role) => {
            info!(caller, login = user.login, repo, role, "someone who steers a session may push to its repo");
            Ok(())
        }
        Err(r) => Err(match r.clear {
            true => format!("{who} can't push to {repo} on GitHub: {}", r.why),
            false => format!("couldn't check {who}'s access to {repo} on GitHub: {}", r.why),
        }),
    }
}

/// Who can steer session `s` besides its asker (module docs), each named
/// for a refusal: its project's owners and editors now, and the GitHub
/// reviewers whose reviews were sent to it.
enum Steerer {
    Member { account: String, who: String },
    Reviewer { user: GithubUser, who: String },
}

async fn steerers(app: &Platform, s: &crate::db::sessions::AgentSession) -> Result<Vec<Steerer>, String> {
    let mut out = Vec::new();
    let members = match crate::boxes::members(app, &s.project).await {
        Ok(Some(m)) => m,
        Ok(None) => return Err("the session's project is gone".into()),
        Err(e) => {
            warn!(session = s.id, error = format!("{e:#}"), "listing who steers a session before its push");
            return Err("couldn't check who can steer the session, so it can't push now".into());
        }
    };
    for m in members {
        if m.account == s.asker || !matches!(m.role.as_str(), "owner" | "editor") {
            continue;
        }
        let name = if m.name.trim().is_empty() { "someone" } else { m.name.trim() };
        out.push(Steerer::Member { account: m.account, who: format!("{name} ({})", m.role) });
    }
    let reviews = app.db.session_feedback(&s.id).map_err(|e| {
        warn!(session = s.id, error = format!("{e:#}"), "reading a session's reviews before its push");
        "couldn't read the session's reviews, so it can't push now".to_owned()
    })?;
    let mut seen = std::collections::HashSet::new();
    for fb in reviews.into_iter().filter(|f| f.kind == FeedbackKind::Review && f.state == FeedbackState::Sent) {
        let login = fb.login.unwrap_or_default();
        let who = format!("the GitHub reviewer {login}");
        let Some(id) = fb.github_id.filter(|_| !login.is_empty()) else {
            return Err(format!(
                "couldn't tell who wrote a review sent to the session, so it can't push to {}",
                s.repo.as_deref().unwrap_or("its repo")
            ));
        };
        if seen.insert(id) {
            out.push(Steerer::Reviewer { user: GithubUser { id, login }, who });
        }
    }
    Ok(out)
}

/// Whether everyone who can steer session `s` may push to `link` now
/// (module docs): its asker first, then the rest. `Err` is the first why
/// not.
async fn steerers_push(
    app: &Platform,
    gh: &GithubApp,
    s: &crate::db::sessions::AgentSession,
    link: &crate::db::forge::LinkedRepo,
) -> Result<(), String> {
    let repo = link.full_name.as_str();
    if s.asker.is_empty() {
        return Err(format!("the session's asker is gone, and it pushes to {repo} as them"));
    }
    account_pushes(app, gh, &s.asker, "the asker", link).await?;
    for st in steerers(app, s).await? {
        let checked = match st {
            Steerer::Member { account, who } => account_pushes(app, gh, &account, &who, link).await,
            Steerer::Reviewer { user, who } => github_pushes(app, gh, &s.asker, &user, &who, link).await,
        };
        checked.map_err(|why| format!("{why}. {STEERERS}"))?;
    }
    Ok(())
}

/// `POST /api/hosted/work/session/push` from `m`: module docs.
pub async fn token(app: &Platform, m: &HostedMachine, a: &PushAsk) -> anyhow::Result<PushGiven> {
    let Some(s) = app.db.session(&a.session)? else { return Ok(refused(m, a, "no such session")) };
    if s.project != m.project || s.workspace != m.workspace || s.machine.as_deref() != Some(m.machine.as_str()) {
        return Ok(refused(m, a, "that session isn't this machine's"));
    }
    if matches!(s.state, State::Failed | State::Stopped) {
        return Ok(refused(m, a, &format!("the session is {}", s.state.as_str())));
    }
    if s.role == crate::db::sessions::Role::Review && !a.clone {
        return Ok(refused(m, a, "a review pushes nothing"));
    }
    if a.branch != s.branch {
        return Ok(refused(m, a, "a session pushes its own branch and nothing else"));
    }
    let Some(repo) = s.repo.as_deref() else { return Ok(refused(m, a, "the session has no repo")) };
    let Some(link) = app.db.linked_repo_named(repo)?.filter(|l| l.project_id == s.project && l.unlinked_at.is_none())
    else {
        return Ok(refused(m, a, &format!("{repo} isn't linked to the session's project")));
    };
    let Some(gh) = app.github.as_ref() else { return Ok(refused(m, a, "this control has no GitHub App")) };
    // Only where everyone who steers it may push on GitHub now.
    if !a.clone
        && let Err(why) = steerers_push(app, gh, &s, &link).await
    {
        return Ok(refused(m, a, &why));
    }
    let who = if s.asker.is_empty() { "platform" } else { s.asker.as_str() };
    let (caller, repo_of) = (Caller::of(app, who), Repo::Id(link.repo_id));
    let minted = match a.clone {
        true => gh.session_clone_token(&caller, link.installation_id, repo_of).await,
        false => gh.session_push_token(&caller, link.installation_id, repo_of).await,
    };
    match minted {
        Ok((token, expires_at)) => {
            // Which session, branch and repo; never the token.
            let clone = a.clone;
            info!(session = s.id, machine = m.machine, branch = s.branch, repo, clone, "a session's push token minted");
            Ok(PushGiven::Given { token, expires_at, revoke: gh.revoke_url() })
        }
        Err(e) => {
            warn!(session = s.id, repo, error = format!("{e:#}"), "no push token for a session");
            Ok(PushGiven::Refused { why: format!("GitHub didn't mint a push token: {e:#}") })
        }
    }
}
