//! Who may read a linked repo's items (M165, #1211): the one check every
//! read of the forge mirror goes through. **No item is readable through
//! the platform except through [`check`].**
//!
//! The rules, for a [`Viewer`] (a person, a CLI device, or a machine with
//! a live `cloud.drive` grant acting for its owner, as linking takes it)
//! and a repo (a `linked_repos` row):
//! - **An unlinked repo** (`unlinked_at` set) is never readable.
//! - **The project's members only:** a personal project's account, or a
//!   member of the team (any role) for a team's. Another project's items
//!   are no items at all to them.
//! - **A public repo:** any member.
//! - **A private repo:** only if GitHub says this person has access, by
//!   their numeric GitHub id (the hub's: they signed in with GitHub), asked
//!   with a read token ([`GithubApp::may_now`], [`Purpose::Read`]). A member
//!   who didn't sign in with GitHub sees nothing private.
//!
//! **Kept answers.** GitHub's answers are kept [`KEEP_MS`] (10 minutes) in
//! `forge_access`, so every machine of the platform gives the same one.
//! The App's `member`, `membership`, `organization`, `repository`, `team`,
//! `team_add` and installation events ([`event`]) drop the answers they
//! could change, for the repo, the owner or the person, on every machine
//! (`forge_access_drops`). An answer asked before a drop is never used
//! after it, even one that came back from GitHub after the drop: a
//! removed collaborator is cut off at the next event.
//!
//! **Hard to go around.** [`check`] is the only maker of a [`Readable`]
//! (its fields are private here), and every `Db` function that returns
//! forge rows to a person takes one and returns rows of its repos only
//! (`Db::forge_items`, `Db::forge_item`, `Db::forge_checks`,
//! `Db::forge_events`). Content (M166), drafts (M170), the inbox (M171)
//! and search add their reads the same way: a function that returns
//! forge rows for a person takes a `&Readable`. A title that goes out in a
//! push or an inbox goes only to those [`title_readers`] names.
//!
//! ```no_run
//! # use arugula_platform::forge::viewer::Readable;
//! fn repos(r: &Readable) -> &[u64] {
//!     r.repos()
//! }
//! ```
//! ```compile_fail
//! # use arugula_platform::forge::viewer::Readable;
//! let anything = Readable { repos: vec![77] };
//! ```

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::ApiError;
use arugula_e2e::now_ms;
use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use serde_json::Value;
use tracing::info;

use super::{Caller, FORGE_LOOKUPS, GithubApp, GithubUser};
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::forge::{AccessScope, LinkedRepo},
};

/// How long GitHub's answer about a person's access is used.
pub const KEEP_MS: u64 = 10 * 60 * 1000;

/// Who is reading: an account, and its GitHub identity if it signed in to
/// control with GitHub. As a route's extractor, a person, a CLI device or
/// a machine with a live `cloud.drive` grant (for its owner).
#[derive(Debug, Clone)]
pub struct Viewer {
    pub account: String,
    pub github: Option<GithubUser>,
}

impl Viewer {
    /// The account as a viewer, with its GitHub identity from the hub.
    pub async fn of(app: &Platform, account: &str) -> Result<Self, ApiError> {
        Ok(Self { account: account.to_owned(), github: app.hub.github_user(account).await? })
    }
}

impl FromRequestParts<Arc<Platform>> for Viewer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, ApiError> {
        let CloudDriver(Person { account }) = CloudDriver::from_request_parts(parts, app).await?;
        Self::of(app, &account).await
    }
}

/// The repos a viewer may read now, as [`check`] found: only it makes one.
/// Every read of forge rows for a person takes one (module docs).
#[derive(Debug)]
pub struct Readable {
    repos: Vec<u64>,
}

impl Readable {
    /// GitHub's ids of the repos, in order.
    pub fn repos(&self) -> &[u64] {
        &self.repos
    }

    pub fn has(&self, repo_id: u64) -> bool {
        self.repos.binary_search(&repo_id).is_ok()
    }

    /// For the database's own tests of the guarded reads.
    #[cfg(test)]
    pub(crate) fn of_repos(mut repos: Vec<u64>) -> Self {
        repos.sort_unstable();
        Self { repos }
    }
}

/// Why a repo's items aren't readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    Unlinked,
    /// Not a member of the repo's project.
    NotMember,
    /// A private repo, and the viewer didn't sign in with GitHub.
    NoGithub,
    /// A private repo, and GitHub doesn't give them access.
    NoAccess,
    /// A private repo not asked about lately, and the account asked about
    /// too many ([`FORGE_LOOKUPS`]).
    Busy,
    /// A private repo, and this control has no GitHub App to ask with.
    NoApp,
}

impl Withheld {
    /// As the wire says it (`WithheldRepo::why`).
    pub fn as_str(self) -> &'static str {
        match self {
            Withheld::Unlinked => "unlinked",
            Withheld::NotMember => "not_member",
            Withheld::NoGithub => "no_github",
            Withheld::NoAccess => "no_access",
            Withheld::Busy => "busy",
            Withheld::NoApp => "no_app",
        }
    }
}

/// What [`check`] found: the readable repos, and each other one's why.
#[derive(Debug)]
pub struct Checked {
    pub readable: Readable,
    pub withheld: Vec<(u64, Withheld)>,
}

/// Whether the account is a member of the project (any role).
async fn member(app: &Platform, account: &str, project: &str) -> Result<bool, ApiError> {
    match super::links::membership(app, account, project, false).await {
        Ok(_) => Ok(true),
        Err(ApiError(StatusCode::NOT_FOUND, ..)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// The one viewer check (module docs): which of `repos` the viewer may
/// read now.
pub async fn check(app: &Platform, v: &Viewer, repos: &[LinkedRepo]) -> Result<Checked, ApiError> {
    let now = now_ms();
    let kept = match &v.github {
        Some(u) => app.db.kept_access(u.id, now.saturating_sub(KEEP_MS))?,
        None => HashMap::new(),
    };
    let mut members: HashMap<&str, bool> = HashMap::new();
    let mut readable = Vec::new();
    let mut withheld = Vec::new();
    let mut ask = Vec::new();
    for r in repos {
        if r.unlinked_at.is_some() {
            withheld.push((r.repo_id, Withheld::Unlinked));
            continue;
        }
        let m = match members.get(r.project_id.as_str()) {
            Some(m) => *m,
            None => *members.entry(&r.project_id).or_insert(member(app, &v.account, &r.project_id).await?),
        };
        if !m {
            withheld.push((r.repo_id, Withheld::NotMember));
            continue;
        }
        if !r.private {
            readable.push(r.repo_id);
            continue;
        }
        let Some(user) = &v.github else {
            withheld.push((r.repo_id, Withheld::NoGithub));
            continue;
        };
        match (kept.get(&r.repo_id), &app.github) {
            (Some(true), _) => readable.push(r.repo_id),
            (Some(false), _) => withheld.push((r.repo_id, Withheld::NoAccess)),
            (None, None) => withheld.push((r.repo_id, Withheld::NoApp)),
            (None, Some(gh)) => ask.push(github_says(app, gh, &v.account, user, r)),
        }
    }
    for (repo_id, says) in futures_util::future::join_all(ask).await {
        match says? {
            None => readable.push(repo_id),
            Some(why) => withheld.push((repo_id, why)),
        }
    }
    readable.sort_unstable();
    readable.dedup();
    Ok(Checked { readable: Readable { repos: readable }, withheld })
}

/// Ask GitHub whether `user` has access to a private repo now, and keep
/// the answer, as of before it asked. `None` when they do.
async fn github_says(
    app: &Platform,
    gh: &GithubApp,
    account: &str,
    user: &GithubUser,
    r: &LinkedRepo,
) -> (u64, anyhow::Result<Option<Withheld>>) {
    if app.limits.check_account(FORGE_LOOKUPS, account).is_err() {
        return (r.repo_id, Ok(Some(Withheld::Busy)));
    }
    let asked = now_ms();
    let yes = gh.may_now(&Caller::of(app, account), user, &r.full_name).await.is_ok();
    let owner = r.full_name.split('/').next().unwrap_or_default();
    let kept = app.db.keep_access(user.id, r.repo_id, owner, yes, asked);
    (r.repo_id, kept.map(|()| (!yes).then_some(Withheld::NoAccess)))
}

/// The readable repos of one item, and its repo: what a read of an item
/// starts with. `None` for an item that isn't there or that the viewer
/// may not read, which is the same to them.
pub async fn item(app: &Platform, v: &Viewer, id: &str) -> Result<Option<(Readable, LinkedRepo)>, ApiError> {
    let Some(repo_id) = app.db.forge_item_repo(id)? else { return Ok(None) };
    let Some(repo) = app.db.linked_repo(repo_id)? else { return Ok(None) };
    let c = check(app, v, std::slice::from_ref(&repo)).await?;
    Ok(c.readable.has(repo_id).then_some((c.readable, repo)))
}

/// Which of `accounts` may see an item's title (in a push or an inbox,
/// M171): those [`check`] lets read its repo. None for an item that
/// isn't there.
pub async fn title_readers(app: &Platform, item: &str, accounts: &[String]) -> Result<Vec<String>, ApiError> {
    let Some(repo_id) = app.db.forge_item_repo(item)? else { return Ok(vec![]) };
    let Some(repo) = app.db.linked_repo(repo_id)? else { return Ok(vec![]) };
    let mut out = Vec::new();
    for a in accounts {
        let v = Viewer::of(app, a).await?;
        if check(app, &v, std::slice::from_ref(&repo)).await?.readable.has(repo_id) {
            out.push(a.clone());
        }
    }
    Ok(out)
}

/// What an event of the App's could change about who may read a repo, as
/// drops: by repo, owner or person. Nothing for any other event.
fn drops(event: &str, v: &Value) -> Vec<(AccessScope, String)> {
    let mut out = Vec::new();
    let mut add = |scope, s: Option<String>| {
        if let Some(s) = s.filter(|s| !s.is_empty()) {
            out.push((scope, s));
        }
    };
    let id = |v: &Value| v.as_u64().map(|n| n.to_string());
    let login = |v: &Value| v.as_str().map(str::to_lowercase);
    match event {
        // A collaborator added, removed or changed; a repo made private or
        // public, renamed, transferred, archived or deleted.
        "member" | "repository" => {
            add(AccessScope::Repo, id(&v["repository"]["id"]));
            add(AccessScope::Person, id(&v["member"]["id"]));
        }
        // Someone joined or left a team or an organization; a team's repos
        // changed; an organization renamed or deleted.
        "membership" | "organization" | "team" | "team_add" => {
            add(AccessScope::Owner, login(&v["organization"]["login"]));
            add(AccessScope::Owner, login(&v["changes"]["login"]["from"]));
            add(AccessScope::Person, id(&v["member"]["id"]));
            add(AccessScope::Person, id(&v["membership"]["user"]["id"]));
            add(AccessScope::Repo, id(&v["repository"]["id"]));
        }
        "installation" | "installation_repositories" => {
            add(AccessScope::Owner, login(&v["installation"]["account"]["login"]));
        }
        _ => {}
    }
    out
}

/// A webhook's event (M161's handler): drop the access answers it could
/// change, on every machine, and the App's own (`standing`'s); and a
/// linked repo made private or public says so from now on.
pub fn event(app: &Platform, event: &str, v: &Value, now: u64) -> anyhow::Result<()> {
    let drops = drops(event, v);
    if drops.is_empty() {
        return Ok(());
    }
    if event == "repository"
        && let (Some(repo_id), Some(private)) = (
            v["repository"]["id"].as_u64(),
            match v["action"].as_str() {
                Some("privatized") => Some(true),
                Some("publicized") => Some(false),
                _ => None,
            },
        )
        && app.db.set_repo_private(repo_id, private)?
    {
        info!(repo = repo_id, private, "a linked repo's visibility changed");
    }
    for (scope, subject) in &drops {
        app.db.drop_access(*scope, subject, now)?;
    }
    if let Some(gh) = &app.github {
        let repo = v["repository"]["full_name"].as_str().map(str::to_lowercase);
        gh.forget_where(|user, name| {
            drops.iter().any(|(scope, s)| match scope {
                AccessScope::Repo => repo.as_deref() == Some(name),
                AccessScope::Owner => name.split('/').next() == Some(s.as_str()),
                AccessScope::Person => user.to_string() == *s,
            })
        });
    }
    info!(event, drops = drops.len(), "dropped kept GitHub access answers");
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn what_each_event_drops() {
        let member = json!({ "action": "removed", "member": { "id": 1002, "login": "bob-gh" },
            "repository": { "id": 77, "full_name": "acme/app" } });
        assert_eq!(drops("member", &member), [(AccessScope::Repo, "77".into()), (AccessScope::Person, "1002".into())]);
        let org = json!({ "action": "member_removed", "organization": { "login": "Acme" },
            "membership": { "user": { "id": 1002 } } });
        assert_eq!(
            drops("organization", &org),
            [(AccessScope::Owner, "acme".into()), (AccessScope::Person, "1002".into())]
        );
        let renamed = json!({ "action": "renamed", "organization": { "login": "acme2" }, "changes": { "login": { "from": "acme" } } });
        assert_eq!(
            drops("organization", &renamed),
            [(AccessScope::Owner, "acme2".into()), (AccessScope::Owner, "acme".into())]
        );
        let team = json!({ "action": "removed", "organization": { "login": "acme" }, "member": { "id": 7 } });
        assert_eq!(
            drops("membership", &team),
            [(AccessScope::Owner, "acme".into()), (AccessScope::Person, "7".into())]
        );
        let added = json!({ "action": "added", "installation": { "id": 9, "account": { "login": "acme" } } });
        assert_eq!(drops("installation_repositories", &added), [(AccessScope::Owner, "acme".into())]);
        for e in ["pull_request", "issues", "push", "ping"] {
            assert!(drops(e, &member).is_empty(), "{e}");
        }
    }
}
