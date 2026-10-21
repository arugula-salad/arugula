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
//!   with a read token ([`GithubApp::may`], [`Purpose::Read`]). A member
//!   who didn't sign in with GitHub sees nothing private.
//!
//! **Kept answers.** GitHub's answers are kept [`KEEP_MS`] (10 minutes) in
//! `forge_access`, so every machine of the platform gives the same one
//! ([`access`]: daemons' live updates and hosted boxes' tokens ask the
//! same way). Answers and drops are timed by the database's clock, never
//! a machine's.
//! The App's `member`, `membership`, `organization`, `repository`, `team`,
//! `team_add` and installation events ([`event`]) drop the answers they
//! could change, for the repo, the owner or the person, on every machine
//! (`forge_access_drops`). An answer asked before a drop is never used
//! after it, even one that came back from GitHub after the drop: a
//! removed collaborator is cut off at the next event. Those events need
//! the App subscribed to them, and organization **Members: read**
//! (`docs/control-ops.md`). Any payload with a repository marks a linked
//! repo private when GitHub says it is.
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
    let kept = match &v.github {
        Some(u) => app.db.kept_access(u.id, KEEP_MS)?,
        None => HashMap::new(),
    };
    let mut members: HashMap<&str, bool> = HashMap::new();
    let mut readable = Vec::new();
    let mut withheld = Vec::new();
    let mut asks = Vec::new();
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
        match (kept.get(&r.full_name.to_lowercase()), &app.github) {
            (Some(None), _) => readable.push(r.repo_id),
            (Some(Some(_)), _) => withheld.push((r.repo_id, Withheld::NoAccess)),
            (None, None) => withheld.push((r.repo_id, Withheld::NoApp)),
            (None, Some(gh)) => {
                asks.push(async move { (r.repo_id, ask(app, gh, &v.account, user, &r.full_name, true).await) });
            }
        }
    }
    for (repo_id, says) in futures_util::future::join_all(asks).await {
        match says? {
            Access::Yes => readable.push(repo_id),
            Access::No(_) => withheld.push((repo_id, Withheld::NoAccess)),
            Access::Busy => withheld.push((repo_id, Withheld::Busy)),
        }
    }
    readable.sort_unstable();
    readable.dedup();
    Ok(Checked { readable: Readable { repos: readable }, withheld })
}

/// What GitHub says of a person's access to a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Yes,
    /// Why not, for the person.
    No(String),
    /// Not asked about lately, and the account asked about too many
    /// ([`FORGE_LOOKUPS`]).
    Busy,
}

/// Whether GitHub gives `user` access to `repo` (`OWNER/NAME`): the answer
/// kept in the database (asked less than [`KEEP_MS`] ago and not dropped
/// since), or asked now and kept. The one way the platform asks: the
/// viewer check, daemons' live updates (`forge::standing`) and hosted
/// boxes' tokens (`forge::token`), so a drop on any machine reaches them
/// all. `limit`: a repository not kept counts against the account's
/// [`FORGE_LOOKUPS`].
pub async fn access(
    app: &Platform,
    gh: &GithubApp,
    account: &str,
    user: &GithubUser,
    repo: &str,
    limit: bool,
) -> anyhow::Result<Access> {
    if let Some(why) = app.db.kept_access(user.id, KEEP_MS)?.remove(&repo.to_lowercase()) {
        return Ok(why.map_or(Access::Yes, Access::No));
    }
    ask(app, gh, account, user, repo, limit).await
}

/// Ask GitHub now ([`GithubApp::may`], with a read token) and keep the
/// answer, timed by the database's clock before asking: a drop that comes
/// while it asks wins.
async fn ask(
    app: &Platform,
    gh: &GithubApp,
    account: &str,
    user: &GithubUser,
    repo: &str,
    limit: bool,
) -> anyhow::Result<Access> {
    if limit && app.limits.check_account(FORGE_LOOKUPS, account).is_err() {
        return Ok(Access::Busy);
    }
    let asked = app.db.clock_ms()?;
    let why = gh.may(&Caller::of(app, account), user, repo).await.err();
    if let Some(why) = &why {
        info!(repo, why, "no access to a repository");
    }
    app.db.keep_access(user.id, repo, why.as_deref(), asked)?;
    Ok(why.map_or(Access::Yes, Access::No))
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
/// drops: by repo (`owner/name`, and what it was called before a rename
/// or a transfer), owner or person. Nothing for any other event.
fn drops(event: &str, v: &Value) -> Vec<(AccessScope, String)> {
    let mut out = Vec::new();
    let mut add = |scope, s: Option<String>| {
        if let Some(s) = s.filter(|s| !s.is_empty()) {
            out.push((scope, s.to_lowercase()));
        }
    };
    let id = |v: &Value| v.as_u64().map(|n| n.to_string());
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let repo = &v["repository"];
    let (owner, name) = (repo["owner"]["login"].as_str(), repo["name"].as_str());
    let was = |owner: Option<&str>, name: Option<&str>| Some(format!("{}/{}", owner?, name?));
    match event {
        // A collaborator added, removed or changed; a repo made private or
        // public, renamed, transferred, archived or deleted.
        "member" | "repository" => {
            add(AccessScope::Repo, text(&repo["full_name"]));
            add(AccessScope::Repo, was(owner, v["changes"]["repository"]["name"]["from"].as_str()));
            let from = &v["changes"]["owner"]["from"];
            let old = from["user"]["login"].as_str().or(from["organization"]["login"].as_str());
            add(AccessScope::Repo, was(old, name));
            add(AccessScope::Person, id(&v["member"]["id"]));
        }
        // Someone joined or left a team or an organization; a team's repos
        // changed; an organization renamed or deleted.
        "membership" | "organization" | "team" | "team_add" => {
            add(AccessScope::Owner, text(&v["organization"]["login"]));
            add(AccessScope::Owner, text(&v["changes"]["login"]["from"]));
            add(AccessScope::Person, id(&v["member"]["id"]));
            add(AccessScope::Person, id(&v["membership"]["user"]["id"]));
            add(AccessScope::Repo, text(&repo["full_name"]));
        }
        "installation" | "installation_repositories" => {
            add(AccessScope::Owner, text(&v["installation"]["account"]["login"]));
        }
        _ => {}
    }
    out
}

/// A webhook's event (M161's handler), whatever it is:
/// - **A repository made private** is private here at once: every payload
///   that carries a repository says whether it's private, a pull request's
///   or a check's as much as `repository`'s own. Only `repository`'s
///   `publicized` makes a linked repo public again, so an older delivery
///   taken late never opens one up (M164's reconcile catches up the rest).
/// - **What changes who may read a repo** drops the access answers it
///   could change, on every machine ([`drops`]).
pub fn event(app: &Platform, event: &str, v: &Value) -> anyhow::Result<()> {
    let repo_id = v["repository"]["id"].as_u64();
    if let Some(id) = repo_id
        && v["repository"]["private"].as_bool() == Some(true)
        && app.db.set_repo_private(id, true)?
    {
        info!(repo = id, event, "a linked repo is private now");
    }
    if let Some(id) = repo_id
        && event == "repository"
        && v["action"].as_str() == Some("publicized")
        && app.db.set_repo_private(id, false)?
    {
        info!(repo = id, "a linked repo is public now");
    }
    let drops = drops(event, v);
    for (scope, subject) in &drops {
        app.db.drop_access(*scope, subject)?;
    }
    if !drops.is_empty() {
        info!(event, drops = drops.len(), "dropped kept GitHub access answers");
    }
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
        assert_eq!(
            drops("member", &member),
            [(AccessScope::Repo, "acme/app".into()), (AccessScope::Person, "1002".into())]
        );
        let moved = json!({ "action": "renamed", "repository": { "id": 77, "full_name": "Acme/App2", "name": "App2",
            "owner": { "login": "Acme" } }, "changes": { "repository": { "name": { "from": "app" } } } });
        assert_eq!(
            drops("repository", &moved),
            [(AccessScope::Repo, "acme/app2".into()), (AccessScope::Repo, "acme/app".into())]
        );
        let moved = json!({ "action": "transferred", "repository": { "id": 77, "full_name": "beta/app", "name": "app",
            "owner": { "login": "beta" } }, "changes": { "owner": { "from": { "organization": { "login": "acme" } } } } });
        assert_eq!(
            drops("repository", &moved),
            [(AccessScope::Repo, "beta/app".into()), (AccessScope::Repo, "acme/app".into())]
        );
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
