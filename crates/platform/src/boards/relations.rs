//! Relations between work items (Arugula #403): `blocks`, `before`,
//! `relates`, `duplicate_of` and `discovered_from` edges between a project's
//! issues and mirrored items, and what they say on a board. Only `blocks` gates
//! ([`wire::RelationKind::blocks_ready`]): a row whose item has an open
//! blocker carries a `blocked_by` mark and is not in the board's ready set,
//! and stays in its lane, for a mark is information and not a lane.
//!
//! **Who may write:** owners and editors of the project (a viewer reads);
//! an agent acts for its owner, whose role is the
//! ceiling, and needs its machine's `issues.write` grant over the project
//! (the edge is on the items, not on a board). Both ends are items of the
//! project that the writer may read, so an edge can't be used to find out
//! whether an item exists. A self-edge and a cycle of `blocks` and `before` edges are refused
//! ([`crate::db::boards_relations`]). An edge from Plane is read-only here.
//! An agent takes off only edges its own machine made.
//!
//! **Who may see:** the ends an edge has are worked out on every read for
//! the reader. The state of a blocker is a fact whoever may read it, so a
//! row is blocked for everyone; the number, title and repo of a blocker go
//! only to a reader who may read it. For one who may not, the row says it is
//! blocked by "something you can't see" (one mark, however many), and the
//! drawer lists the edge without its other end.

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::boards as wire;
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{
    Facts, J, bad, boards_changed, id_ok,
    lanes::{Fact, Pr},
    refused,
};
use crate::{
    Platform,
    auth::{Machine, Person},
    db::{
        boards_relations::{NewRelation, Put, RelRow},
        issues::Actor,
        projects::ISSUES_WRITE,
    },
    grants::{AgentScope, agent_may},
};

/// The project `asked` as `by` may change its relations: a person's role is
/// the ceiling, an agent needs `issues.write` too.
async fn place(app: &Platform, by: Actor<'_>, asked: &str) -> Result<String, ApiError> {
    let id = super::place(app, Actor::person(by.account), asked, true).await?;
    if let Some(machine) = by.agent {
        let m = Machine { account: by.account.to_owned(), daemon: machine.to_owned() };
        agent_may(app, &m, ISSUES_WRITE, &AgentScope::Project(id.clone())).await?;
    }
    Ok(id)
}

/// `id` is an issue's or a mirrored item's id.
fn end_ok(id: &str) -> Result<(), ApiError> {
    id_ok(id, "iss").or_else(|_| id_ok(id, "fi")).map_err(|_| bad("an item is an id that starts iss_ or fi_"))
}

fn wire_of(r: &RelRow) -> wire::Relation {
    wire::Relation {
        id: r.id.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
        kind: r.kind,
        source: r.source,
        created_at: r.created_at,
    }
}

/// Tell the boards that show either end.
fn notify(app: &Platform, project: &str, ends: &[&str]) {
    match app.db.boards_on_pointers(project, ends) {
        Ok(boards) => boards_changed(app, &boards),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on a related item"),
    }
}

/// Relate two items of `project`.
pub async fn add(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    n: &wire::NewRelation,
) -> Result<wire::Relation, ApiError> {
    let project = place(app, by, project).await?;
    match put(app, by, &project, &n.from, &n.to, n.kind).await? {
        Put::Made(r) => Ok(wire_of(&r)),
        Put::There(_) => Err(err(StatusCode::CONFLICT, "that relation is there already")),
    }
}

/// Store the edge `from` `kind` `to`, both ids, in `project` (already
/// placed): what was stored, or the id of the one there already.
async fn put(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    from: &str,
    to: &str,
    kind: wire::RelationKind,
) -> Result<Put, ApiError> {
    end_ok(from)?;
    end_ok(to)?;
    if from == to {
        return Err(bad("an item can't relate to itself"));
    }
    let (mut items, mut issues): (Vec<&str>, Vec<&str>) = (vec![], vec![]);
    for id in [from, to] {
        if id.starts_with("iss_") { issues.push(id) } else { items.push(id) }
    }
    let known = super::known(app, by.account, project, &items, &issues).await?;
    for id in [from, to] {
        if !known.contains_key(id) {
            return Err(bad(&format!("{id} is not an item of this project that you can read")));
        }
    }
    let new = NewRelation {
        from,
        to,
        kind,
        source: if by.agent.is_some() { wire::RelationSource::Agent } else { wire::RelationSource::Person },
        created_by: by.account,
        agent: by.agent,
    };
    let made = app.db.relation_put(project, &new, now_ms())?.map_err(refused)?;
    if matches!(made, Put::Made(_)) {
        notify(app, project, &[from, to]);
    }
    Ok(made)
}

/// Take a relation off `project`. A person takes off any made here; an
/// agent, those its own machine made; none that comes from Plane or GitHub.
pub async fn remove(app: &Platform, by: Actor<'_>, project: &str, id: &str) -> Result<(), ApiError> {
    let project = place(app, by, project).await?;
    id_ok(id, "rel").map_err(|_| bad("expected a relation id that starts rel_"))?;
    if let Some(r) = app.db.relation(&project, id)? {
        own_machine_only(by, &r)?;
    }
    let gone = app.db.relation_delete(&project, id)?.map_err(refused)?;
    notify(app, &project, &[&gone.from, &gone.to]);
    Ok(())
}

/// An agent takes off only the relations its own machine made; none that
/// comes from Plane or GitHub is a machine's to judge (the delete refuses it).
fn own_machine_only(by: Actor<'_>, r: &RelRow) -> Result<(), ApiError> {
    if let Some(machine) = by.agent
        && r.agent.as_deref() != Some(machine)
        && matches!(r.source, wire::RelationSource::Person | wire::RelationSource::Agent)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent takes off only the relations its own machine made: this one is someone else's",
        ));
    }
    Ok(())
}

/// Links added and removed together (`link`), each on its own: the answer
/// says what became of every one. A reference is anything
/// [`super::resolve::one`] takes. A link that is refused (an end that isn't
/// an item you can read, a cycle, someone else's) leaves the rest to land.
pub async fn batch(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    b: &wire::LinkBatch,
) -> Result<wire::LinksDone, ApiError> {
    if b.add.len() + b.remove.len() > wire::LINKS_MAX {
        return Err(bad(&format!("at most {} links at a time", wire::LINKS_MAX)));
    }
    let project = place(app, by, project).await?;
    let mut done = wire::LinksDone::default();
    for l in &b.add {
        done.add.push(settle(link_add(app, by, &project, l).await)?);
    }
    for l in &b.remove {
        done.remove.push(settle(link_remove(app, by, &project, l).await)?);
    }
    Ok(done)
}

/// A refusal (4xx) is that link's result; anything else fails the call.
fn settle(r: Result<wire::LinkOutcome, ApiError>) -> Result<wire::LinkResult, ApiError> {
    match r {
        Ok(outcome) => Ok(wire::LinkResult { outcome, why: None }),
        Err(ApiError(status, why, _)) if status.is_client_error() => {
            Ok(wire::LinkResult { outcome: wire::LinkOutcome::Refused, why: Some(why) })
        }
        Err(e) => Err(e),
    }
}

/// The ids a link's two references name in `project`.
async fn ends(app: &Platform, by: Actor<'_>, project: &str, l: &wire::LinkEnds) -> Result<(String, String), ApiError> {
    let mut ids = vec![];
    for r in [&l.from, &l.to] {
        match super::resolve::one(app, by.account, project, r).await? {
            Ok(wire::Pointer::Issue { id } | wire::Pointer::ForgeItem { id }) => ids.push(id),
            Ok(_) => return Err(bad(&format!("{}: not an issue or PR", r.trim()))),
            Err(why) => return Err(bad(&format!("{}: {why}", r.trim()))),
        }
    }
    let to = ids.pop().unwrap_or_default();
    Ok((ids.pop().unwrap_or_default(), to))
}

async fn link_add(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    l: &wire::LinkEnds,
) -> Result<wire::LinkOutcome, ApiError> {
    let (from, to) = ends(app, by, project, l).await?;
    Ok(match put(app, by, project, &from, &to, l.kind).await? {
        Put::Made(_) => wire::LinkOutcome::Added,
        Put::There(_) => wire::LinkOutcome::AlreadyThere,
    })
}

async fn link_remove(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    l: &wire::LinkEnds,
) -> Result<wire::LinkOutcome, ApiError> {
    let (from, to) = ends(app, by, project, l).await?;
    let both = matches!(l.kind, wire::RelationKind::Relates);
    let found = match app.db.relation_between(project, &from, &to, l.kind)? {
        Some(r) => Some(r),
        None if both => app.db.relation_between(project, &to, &from, l.kind)?,
        None => None,
    };
    let Some(r) = found else { return Ok(wire::LinkOutcome::NotThere) };
    own_machine_only(by, &r)?;
    let gone = app.db.relation_delete(project, &r.id)?.map_err(refused)?;
    notify(app, project, &[&gone.from, &gone.to]);
    Ok(wire::LinkOutcome::Removed)
}

/// The edges of a board's items, as one reader sees them.
pub(super) struct Edges {
    rels: Vec<RelRow>,
    /// The reader is an owner or editor of the project.
    editor: bool,
}

impl Edges {
    /// The edges that touch `ids`, the ids of a board's items.
    pub(super) async fn load(app: &Platform, reader: &str, project: &str, ids: &[&str]) -> Result<Edges, ApiError> {
        let rels = app.db.relations_among(project, ids)?;
        let editor =
            matches!(super::asks::role_in(app, project, reader).await?, Some(TeamRole::Owner | TeamRole::Editor));
        Ok(Edges { rels, editor })
    }

    /// The pointers at the other ends, for the read to look up: their state
    /// and, if the reader may read them, their chips.
    pub(super) fn others(&self, ids: &[&str]) -> Vec<wire::Pointer> {
        let mut out: Vec<wire::Pointer> = vec![];
        for r in &self.rels {
            for end in [&r.from, &r.to] {
                if !ids.contains(&end.as_str()) {
                    out.push(pointer_of(end));
                }
            }
        }
        out
    }

    /// What `item`'s edges say on its row: the drawer's relations, the
    /// `blocked_by` marks, and whether an open blocker is on it.
    pub(super) fn of(
        &self,
        item: &str,
        facts: &Facts,
        live: &HashMap<String, wire::Live>,
    ) -> (Vec<wire::RowRelation>, Vec<wire::Mark>) {
        let mut shown = vec![];
        let mut marks = vec![];
        let mut hidden = false;
        for r in self.rels.iter().filter(|r| r.from == item || r.to == item) {
            let outward = r.from == item;
            let other = if outward { &r.to } else { &r.from };
            let seen = live.get(other.as_str());
            let removable =
                self.editor && matches!(r.source, wire::RelationSource::Person | wire::RelationSource::Agent);
            shown.push(wire::RowRelation {
                id: r.id.clone(),
                kind: r.kind,
                direction: if outward { wire::RelationDirection::Out } else { wire::RelationDirection::In },
                source: r.source,
                other: seen.cloned(),
                item: seen.map(|_| other.clone()),
                removable,
            });
            if outward || !r.kind.blocks_ready() || !is_open(facts.by_id.get(other.as_str())) {
                continue;
            }
            match seen {
                Some(l) => marks.push(wire::Mark::BlockedBy {
                    number: Some(l.number),
                    title: Some(l.title.clone()),
                    repo: l.repo.clone(),
                }),
                None if !hidden => {
                    hidden = true;
                    marks.push(wire::Mark::BlockedBy { number: None, title: None, repo: None });
                }
                None => {}
            }
        }
        (shown, marks)
    }
}

/// The pointer an item id is.
fn pointer_of(id: &str) -> wire::Pointer {
    if id.starts_with("iss_") {
        wire::Pointer::Issue { id: id.to_owned() }
    } else {
        wire::Pointer::ForgeItem { id: id.to_owned() }
    }
}

/// Whether a blocker still blocks: an open issue or an open PR. An item the
/// mirror no longer has (no fact) blocks nothing.
fn is_open(f: Option<&Fact>) -> bool {
    match f {
        Some(Fact::Issue { category, .. }) => !category.is_done(),
        Some(Fact::Pr(Pr::Open)) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------- routes

/// `POST /api/projects/{project}/relations`.
pub async fn add_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(n): Json<wire::NewRelation>,
) -> J<wire::Relation> {
    Ok(Json(add(&app, Actor::person(&s.account), &project, &n).await?))
}

/// `DELETE /api/projects/{project}/relations/{relation}`.
pub async fn remove_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
) -> J<serde_json::Value> {
    remove(&app, Actor::person(&s.account), &project, &id).await?;
    Ok(Json(serde_json::json!({})))
}
