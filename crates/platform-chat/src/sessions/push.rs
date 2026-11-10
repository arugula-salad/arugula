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
//! - its asker may push to that repo on GitHub right now: the hub's
//!   GitHub identity for them, and GitHub's own answer about their role
//!   on the repo (write or better: [`GithubApp::may_push`]), asked for
//!   each push with a read token. A session pushes as the person who
//!   asked for it, so it pushes only where they could. No GitHub identity,
//!   a lesser role, an asker whose account is gone, or GitHub not
//!   answering: refused, with the reason for the box.
//!
//! The token covers that repo alone, with `contents: write` and
//! `workflows: write` (aru-115, amended 2026-10-26). GitHub compares a new
//! branch's tree with the default branch's, so without `workflows` it
//! refuses the push of any session that began before main changed
//! `.github/workflows/`: the branch's older workflow file reads as an
//! edit. So a session's branch may change CI files, and the PR's reviewer
//! checks them (its draft says when it does); and since the asker's own
//! GitHub access now bounds the mint, a session changes a repo's CI files
//! only for someone who could push them to that repo themselves. It's
//! logged as every mint is (`session:push`, for the asker), never kept,
//! and the box revokes it once it pushed. The App's key never leaves the
//! platform.
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
    db::sessions::State,
    forge::{Caller, GithubApp, Repo},
    work::HostedMachine,
};

fn refused(m: &HostedMachine, a: &PushAsk, why: &str) -> PushGiven {
    warn!(session = a.session, machine = m.machine, why, "refused a hosted machine's push token");
    PushGiven::Refused { why: why.into() }
}

/// Whether `asker` may push to the linked repo on GitHub right now
/// (module docs): their GitHub identity from the hub, then GitHub's
/// answer about their role, through the link's installation. `Err` is why
/// not, for the box. Fails closed: no identity, no answer, no token.
async fn asker_pushes(
    app: &Platform,
    gh: &GithubApp,
    asker: &str,
    link: &crate::db::forge::LinkedRepo,
) -> Result<(), String> {
    let repo = link.full_name.as_str();
    if asker.is_empty() {
        return Err(format!("the session's asker is gone, and it pushes to {repo} as them"));
    }
    let user = match app.hub.github_user(asker).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return Err(format!(
                "the asker hasn't signed in to control with GitHub, so the session can't push to {repo}"
            ));
        }
        Err(e) => {
            warn!(asker, error = format!("{e:#}"), "asking the hub for a session asker's GitHub identity");
            return Err(format!("couldn't check the asker's GitHub identity, so the session can't push to {repo} now"));
        }
    };
    match gh.may_push(&Caller::of(app, asker), link.installation_id, link.repo_id, &user, repo).await {
        Ok(role) => {
            info!(asker, login = user.login, repo, role, "a session's asker may push to its repo");
            Ok(())
        }
        Err(r) => Err(match r.clear {
            true => format!("the asker can't push to {repo} on GitHub: {}", r.why),
            false => format!("couldn't check the asker's access to {repo} on GitHub: {}", r.why),
        }),
    }
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
    // A push is the asker's: only where they may push on GitHub now.
    if !a.clone
        && let Err(why) = asker_pushes(app, gh, &s.asker, &link).await
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
