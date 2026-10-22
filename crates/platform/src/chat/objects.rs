//! Object threads (M196): a room on any global id, in the object's
//! project. Kind `object`, scope `project:<id>`, `project_id` that
//! project, no machine, and `target` the id. Made the first time someone
//! opens it (`POST /api/chat/targets`, kind `object`), as a pane's thread
//! is; from then on it's a room like any other: read marks, search,
//! pushes for mentions, the live stream. There's one per object in each
//! project. A mirrored item's thread goes with the item's other rows when
//! its repo's are purged (M163): once an unlinked repo's grace is over, or
//! at once when it's linked to another project.
//!
//! **What an id names** ([`find`]), by its prefix:
//! - `iss_`: a native issue (M194), in its project.
//! - `fi_`: a mirrored GitHub PR or issue (step 4), in its repo's linked
//!   project. A private repo's also needs the reader's own GitHub access
//!   ([`may_read_private`]: M165's check). An unlinked repo's items have
//!   no threads.
//! - `cw_`: a hosted workspace, with step 5's M179: not yet.
//! - Anything else: no such object.
//!
//! **Who sees one:** the third rule beside a team's roster and a machine's
//! reach: its project's members ([`member`]). A personal project's account,
//! or anyone in its team's roster now (a locked team's owners only). Anyone
//! else gets 404, as for chat's other rooms.

use std::collections::HashMap;

use arugula_control_common::{ApiError, err, refusal};
use axum::http::StatusCode;

use super::Facts;
use crate::{
    Platform,
    db::chat::Room,
    forge::viewer::{self, Viewer},
};

/// The scope of an object's thread is its project's.
pub(super) fn project_scope(project: &str) -> String {
    format!("project:{project}")
}

/// The project an object's thread is in.
pub(super) fn project_of(room: &Room) -> Option<&str> {
    room.scope.strip_prefix("project:")
}

/// The reason a refusal gives for a private repo's item.
pub const PRIVATE_REPO: &str = "private_repo";
/// ...and for an object whose threads haven't come yet (a workspace's).
pub const NOT_YET: &str = "not_yet";

/// What a global id names, as an object thread needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Object {
    pub project: String,
    /// The room's name: `issue #12`, `PR owner/name#3`.
    pub name: String,
    /// More than membership guards it.
    pub guard: Guard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Guard {
    /// Its project's members read it.
    Members,
    /// A private repo's item: its members, with their own GitHub access to
    /// the repo too (M165).
    PrivateRepo { repo_id: u64, full_name: String },
}

/// What an id's prefix says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    Issue,
    Mirrored,
    Workspace,
}

fn prefix(target: &str) -> Option<Prefix> {
    let (p, rest) = target.split_once('_')?;
    if rest.is_empty() || rest.len() > 64 || !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    match p {
        "iss" => Some(Prefix::Issue),
        "fi" => Some(Prefix::Mirrored),
        "cw" => Some(Prefix::Workspace),
        _ => None,
    }
}

/// Whether `target` is an object's global id by its shape (`iss_…`,
/// `fi_…`, `cw_…`), not a room's: an agent names an object's thread by it
/// (M198).
pub(super) fn is_object(target: &str) -> bool {
    prefix(target).is_some()
}

/// The objects these ids name, by id: one statement per kind for all of
/// them. An id that names nothing (an unknown prefix, a row that isn't
/// there, an unlinked repo's item, a workspace for now) isn't in the map.
pub(super) fn find(app: &Platform, targets: &[&str]) -> anyhow::Result<HashMap<String, Object>> {
    let mut out = HashMap::new();
    let of = |p| targets.iter().copied().filter(move |t| prefix(t) == Some(p));
    let issues: Vec<&str> = of(Prefix::Issue).collect();
    for (id, project, number) in app.db.issue_places(&issues)? {
        out.insert(id, Object { project, name: format!("issue #{number}"), guard: Guard::Members });
    }
    let mirrored: Vec<&str> = of(Prefix::Mirrored).collect();
    for m in app.db.mirrored_items(&mirrored)? {
        if m.unlinked {
            continue;
        }
        let what = if m.kind == "pr" { "PR" } else { "issue" };
        let guard = if m.private {
            Guard::PrivateRepo { repo_id: m.repo_id, full_name: m.full_name.clone() }
        } else {
            Guard::Members
        };
        out.insert(m.id, Object { project: m.project_id, name: format!("{what} {}#{}", m.full_name, m.number), guard });
    }
    // Hosted workspaces wait on step 5's M179 ([`open`] says so).
    Ok(out)
}

/// The object `target` names, for someone opening its thread: 404 for an
/// id that names nothing, and "not yet" for a workspace.
pub(super) fn resolve(app: &Platform, target: &str) -> Result<Object, ApiError> {
    let nothing = || err(StatusCode::NOT_FOUND, "no such object");
    match prefix(target).ok_or_else(nothing)? {
        Prefix::Workspace => Err(refusal(
            StatusCode::NOT_IMPLEMENTED,
            NOT_YET,
            "a workspace's thread isn't here yet: it comes with hosted workspaces' records (M179)",
        )),
        Prefix::Issue | Prefix::Mirrored => find(app, &[target])?.remove(target).ok_or_else(nothing),
    }
}

/// Whether `account` is a member of `project` now: its account for a
/// personal project, anyone in its team's roster for a team's (a locked
/// team's owners only).
pub(super) fn member(facts: &Facts, project: &str, account: &str) -> bool {
    match facts.projects.get(project) {
        Some(h) if h.account.is_some() => h.account.as_deref() == Some(account),
        Some(h) => h.team.as_deref().is_some_and(|t| facts.role(t, account).is_some()),
        None => false,
    }
}

/// Whether `account`, a member of a private repo's project, may read that
/// repo's item `target`: step 4's own-access check (M165,
/// `forge::viewer::item`), the one every read of a mirrored item takes.
pub(super) async fn may_read_private(
    app: &Platform,
    account: &str,
    target: &str,
    repo_id: u64,
) -> anyhow::Result<bool> {
    let api = |e: ApiError| anyhow::anyhow!("{} {}", e.0, e.1);
    let v = Viewer::of(app, account).await.map_err(api)?;
    Ok(viewer::item(app, &v, target).await.map_err(api)?.is_some_and(|(_, r)| r.repo_id == repo_id))
}

/// Whether `account` sees the thread on `room`'s object, and from when (0:
/// everything). Its project's members, past the object's guard; the
/// object must still name that project.
pub(super) async fn sight(app: &Platform, facts: &Facts, account: &str, room: &Room) -> anyhow::Result<Option<u64>> {
    let (Some(project), Some(target)) = (project_of(room), room.target.as_deref()) else { return Ok(None) };
    let Some(obj) = facts.objects.get(target).filter(|o| o.project == project) else { return Ok(None) };
    if !member(facts, project, account) {
        return Ok(None);
    }
    Ok(match &obj.guard {
        Guard::Members => Some(0),
        Guard::PrivateRepo { repo_id, .. } => may_read_private(app, account, target, *repo_id).await?.then_some(0),
    })
}

/// Whom a mention in an object's thread may reach: its project's members
/// (the caller keeps those who see it).
pub(super) fn audience(facts: &Facts, room: &Room) -> Vec<String> {
    let Some(h) = project_of(room).and_then(|p| facts.projects.get(p)) else { return vec![] };
    match (&h.account, &h.team) {
        (Some(a), _) => vec![a.clone()],
        (None, Some(t)) => {
            facts.teams.get(t).map(|t| t.members.iter().map(|m| m.account.clone()).collect()).unwrap_or_default()
        }
        (None, None) => vec![],
    }
}

/// Whether `account` owns the project an object's thread is in: a
/// personal project's account, or a team's owner. Owners delete others'
/// messages there.
pub(super) fn owns(facts: &Facts, room: &Room, account: &str) -> bool {
    let Some(h) = project_of(room).and_then(|p| facts.projects.get(p)) else { return false };
    match (&h.account, &h.team) {
        (Some(a), _) => a == account,
        (None, Some(t)) => facts.role(t, account) == Some(arugula_e2e::team::TeamRole::Owner),
        (None, None) => false,
    }
}

/// The team whose project an object's thread is in, for search's `team=`.
pub(super) fn team_of<'a>(facts: &'a Facts, room: &Room) -> Option<&'a str> {
    project_of(room).and_then(|p| facts.projects.get(p)).and_then(|h| h.team.as_deref())
}

/// Open the thread on `target` for `account`: 404 unless they're a member
/// of its project, then a private repo's refusal unless GitHub gives them
/// access (M165).
pub(super) async fn check_open(
    app: &Platform,
    facts: &Facts,
    account: &str,
    target: &str,
    obj: &Object,
) -> Result<(), ApiError> {
    if !member(facts, &obj.project, account) {
        return Err(err(StatusCode::NOT_FOUND, "no such object"));
    }
    if let Guard::PrivateRepo { repo_id, full_name } = &obj.guard
        && !may_read_private(app, account, target, *repo_id).await?
    {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            PRIVATE_REPO,
            &format!("{full_name} is private, and GitHub doesn't give you access to it"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_known_by_its_prefix() {
        assert_eq!(prefix("iss_0123abcd"), Some(Prefix::Issue));
        assert_eq!(prefix("fi_00ff"), Some(Prefix::Mirrored));
        assert_eq!(prefix("cw_9d81"), Some(Prefix::Workspace));
        for bad in ["prj_1", "iss_", "fi", "fi_a-b", "_x", "", &format!("fi_{}", "a".repeat(65))] {
            assert_eq!(prefix(bad), None, "{bad}");
        }
    }
}
