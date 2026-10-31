//! Which issue or PR a reference names (#366): `#N`, `owner/repo#N`,
//! `IDENT-N`, or an `iss_`/`fi_` id, in the project of a board. The agent
//! tool writes rows by their items, so the machine asks here before it adds
//! or attaches one. Nothing changes.
//!
//! `#N` is the project's native issue when it has one. Otherwise it is the
//! issue or PR of a linked repo, when exactly one has that number; when
//! several do, it names none and the answer says which, rather than guessing.
//! A repo's item is `owner/repo#N`.

use arugula_control_wire::boards::{self as wire, IssueRef, Pointer};

use super::{J, bad};
use crate::{
    Platform,
    auth::Machine,
    db::{forge::ItemFilter, issues::Actor, trackers::TrackerFilter},
    forge::viewer::{self, Viewer},
};
use arugula_control_common::ApiError;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

/// Plane items read to find one by its number.
const PLANE_SCAN: u32 = 5_000;

/// `POST /api/daemon/boards/{id}/resolve`.
pub async fn route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(r): Json<wire::ResolveItems>,
) -> J<wire::Resolved> {
    app.limits.check_daemon(super::agent::AGENT_BOARD_WRITES, &d.daemon)?;
    if r.items.len() > wire::RESOLVE_MAX {
        return Err(bad(&format!("at most {} references at a time", wire::RESOLVE_MAX)));
    }
    let by = Actor { account: &d.account, agent: Some(&d.daemon) };
    let board = super::board_place(&app, by, &id, true).await?;
    let mut items = Vec::with_capacity(r.items.len());
    for item in r.items {
        let (pointer, why) = match one(&app, &d.account, &board.project, &item).await? {
            Ok(p) => (Some(p), None),
            Err(why) => (None, Some(why)),
        };
        items.push(wire::Resolution { item, pointer, why });
    }
    Ok(Json(wire::Resolved { items }))
}

/// The pointer `item` names in `project`, or why it names none.
pub(crate) async fn one(
    app: &Platform,
    account: &str,
    project: &str,
    item: &str,
) -> Result<Result<Pointer, String>, ApiError> {
    let item = item.trim();
    if item.starts_with("iss_") {
        let ok = app.db.issue(item)?.is_some_and(|i| i.project == project);
        return Ok(if ok {
            Ok(Pointer::Issue { id: item.to_owned() })
        } else {
            Err(format!("{item} isn't an issue of this project"))
        });
    }
    if item.starts_with("fi_") {
        super::id_ok(item, "fi")?;
        return Ok(Ok(Pointer::ForgeItem { id: item.to_owned() }));
    }
    let Some(want) = IssueRef::parse(item) else {
        return Ok(Err("an item is #N, owner/repo#N, IDENT-N or an iss_/fi_ id".to_owned()));
    };
    let repos = app.db.linked_repos(project)?.into_iter().filter(|l| l.unlinked_at.is_none()).collect::<Vec<_>>();
    let viewer = Viewer::of(app, account).await?;
    let checked = viewer::check(app, &viewer, &repos).await?;
    let in_repo = |number: u64| -> Result<Vec<(String, Pointer)>, ApiError> {
        let f = ItemFilter { number: Some(number), limit: 50, ..Default::default() };
        let mut out = vec![];
        for i in app.db.forge_items(&checked.readable, &f)? {
            if let Some(l) = repos.iter().find(|l| l.repo_id == i.repo_id) {
                out.push((format!("{}#{}", l.full_name, i.number), Pointer::ForgeItem { id: i.id }));
            }
        }
        Ok(out)
    };
    Ok(match want {
        IssueRef::Number(n) => {
            let native = app.db.issue_by_number(project, n)?;
            if let Some(i) = native {
                // The project's own issue wins over any repo's item of that number.
                return Ok(Ok(Pointer::Issue { id: i.id }));
            }
            let mut github = in_repo(n)?;
            match github.len() {
                1 => Ok(github.remove(0).1),
                0 => Err(format!("nothing is #{n} in this project")),
                _ => {
                    let names: Vec<String> = github.into_iter().map(|(name, _)| name).collect();
                    Err(format!(
                        "#{n} names more than one thing ({}): write owner/repo#{n} or the iss_/fi_ id",
                        names.join(" and ")
                    ))
                }
            }
        }
        IssueRef::Repo { repo, number } => {
            let found =
                in_repo(number)?.into_iter().find(|(name, _)| name.eq_ignore_ascii_case(&format!("{repo}#{number}")));
            found.map(|f| f.1).ok_or_else(|| format!("nothing is {repo}#{number} in this project"))
        }
        IssueRef::Plane { key, number } => {
            let conns = viewer::check_connections(app, &viewer, &app.db.tracker_connections(project)?).await?;
            let f = TrackerFilter { limit: PLANE_SCAN, ..Default::default() };
            let mut found = None;
            for i in app.db.tracker_items(&conns, &f)? {
                let same = conns.get(i.mirror_id).is_some_and(|c| c.identifier.eq_ignore_ascii_case(&key));
                if same && i.number == number {
                    found = Some(Pointer::ForgeItem { id: i.id });
                    break;
                }
            }
            found.ok_or_else(|| format!("nothing is {key}-{number} in this project"))
        }
    })
}
