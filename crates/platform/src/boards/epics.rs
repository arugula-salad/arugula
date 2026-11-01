//! Plane epics as boards (Arugula #483, #371, aru-124): each open epic of a
//! connection has a board, its children are the board's rows, and the board
//! keeps following the epic.
//!
//! - **Who acts.** The connection does, in control, on its own behalf: a
//!   board it makes has no `created_by` and no agent, and says where it came
//!   from in its `source`. No agent grant is involved.
//! - **[`from_epics`]** runs after a mirror write (the webhook, the
//!   reconcile, the backfill, a write's read-after-write) with the `fi_` ids
//!   the write changed, once the whole page is written, so a child that came
//!   before its epic (or in the same page) finds the same boards as one that
//!   came after. **[`first_pass`]** reads the whole mirror: when the
//!   backfill finishes and when the switch goes on.
//! - **A board follows its epic:** its title; closed when the epic is
//!   completed, cancelled or deleted, or is no longer an epic, open again when the epic is (if the
//!   connection closed it); and the rows the sync added are the epic's
//!   children, found by the mirror's `parent_source_id`. A child that moved
//!   to another epic is taken off the old board by the rows' `source`, since
//!   the mirror only knows its new parent.
//! - **What it never does:** set a lane (they follow the work, aru-125), make
//!   a group, add a row a person added already, or remove a row a person
//!   added. The goal is read once, when the board is made.
//! - **The switch** is the connection's `epics_as_boards`. Off, the boards and
//!   their rows stay and nothing follows; on again, [`first_pass`] catches up.

use std::collections::HashSet;

use arugula_control_wire::boards::ROWS_MAX;
use arugula_e2e::now_ms;
use tracing::warn;

use crate::{
    Platform,
    db::{
        boards_epics::{Follow, NewEpicBoard, Want},
        trackers::{Connection, Node, key_name},
    },
    keys,
    trackers::{self, TrackerError, TrackerProvider},
};

/// The most characters of a goal.
const GOAL_MAX: usize = arugula_control_wire::boards::GOAL_MAX;

/// The goal an epic's description makes: its first line, as plain text
/// (Plane's stripped description joins paragraphs with a single newline).
pub fn goal_of(description: &str) -> String {
    let line = description
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    line.chars().filter(|c| !c.is_control()).take(GOAL_MAX).collect()
}

/// The note a board closes with.
fn done_note(group: &str) -> &'static str {
    if group == "cancelled" { "The epic was cancelled in Plane" } else { "The epic was completed in Plane" }
}

/// Tracker items (`fi_` ids) of `connection` were written or removed: make,
/// follow and fill the boards of the epics among them and of the epics the
/// children among them belong to. Nothing when the connection's switch is
/// off.
pub async fn from_epics(app: &Platform, connection: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    if let Err(e) = run(app, connection, Some(ids)).await {
        warn!(%connection, error = format!("{e:#}"), "boards: following a connection's epics");
    }
}

/// The same for the whole mirror: every epic of the connection.
pub async fn first_pass(app: &Platform, connection: &str) {
    if let Err(e) = run(app, connection, None).await {
        warn!(%connection, error = format!("{e:#}"), "boards: the first pass over a connection's epics");
    }
}

async fn run(app: &Platform, connection: &str, ids: Option<&[String]>) -> anyhow::Result<()> {
    let Some(c) = app.db.live_tracker_connection(connection)? else { return Ok(()) };
    if !c.epics_as_boards {
        return Ok(());
    }
    // Boards are off for whoever connected it: none are made.
    let Some(who) = &c.linked_by else { return Ok(()) };
    if !crate::chat::flag_on(app, who, super::FLAG).await? {
        return Ok(());
    }
    let nodes = app.db.tracker_nodes(&c.id, ids)?;
    let mut epics: Vec<Node> = vec![];
    if let Some(ids) = ids {
        // Rows of children that went, or moved to another epic (or none).
        let mut moved: Vec<(String, Option<String>)> = vec![];
        let present: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        moved.extend(ids.iter().filter(|id| !present.contains(id.as_str())).map(|id| (id.clone(), None)));
        for n in &nodes {
            let parent = match &n.parent_source_id {
                Some(p) => app.db.tracker_node(&c.id, p)?.filter(|p| p.is_epic),
                None => None,
            };
            moved.push((n.id.clone(), parent.as_ref().map(|p| p.id.clone())));
            if let Some(p) = parent
                && !epics.iter().any(|e| e.id == p.id)
            {
                epics.push(p);
            }
        }
        let left = app.db.drop_moved_rows(&moved, now_ms())?;
        super::boards_changed(app, &left);
        // An epic that was deleted: its board closes.
        let gone: Vec<String> = ids.iter().filter(|id| !present.contains(id.as_str())).cloned().collect();
        for (board, _) in app.db.epic_boards(&c.project_id, &gone)? {
            if app.db.follow_epic(&board.id, &board.title, Follow::Closed("The epic was deleted in Plane"), now_ms())? {
                super::changed(app, &board.project, &board.id);
            }
        }
    }
    // An item that stopped being an epic: its board closes and nothing follows it.
    let stopped: Vec<String> = nodes.iter().filter(|n| !n.is_epic).map(|n| n.id.clone()).collect();
    for (board, _) in app.db.epic_boards(&c.project_id, &stopped)? {
        let note = "The item is no longer an epic in Plane";
        if app.db.follow_epic(&board.id, &board.title, Follow::Closed(note), now_ms())? {
            super::changed(app, &board.project, &board.id);
        }
    }
    for n in nodes.into_iter().filter(|n| n.is_epic) {
        if !epics.iter().any(|e| e.id == n.id) {
            epics.push(n);
        }
    }
    for epic in &epics {
        if let Err(e) = follow(app, &c, epic).await {
            warn!(connection = %c.id, error = format!("{e:#}"), "boards: following one epic");
        }
    }
    Ok(())
}

/// Make the epic's board if it is open and has none, then make the board
/// follow it.
async fn follow(app: &Platform, c: &Connection, epic: &Node) -> anyhow::Result<()> {
    let now = now_ms();
    let had = app.db.epic_boards(&c.project_id, std::slice::from_ref(&epic.id))?.into_iter().next();
    let (board, mut changed) = match had {
        Some((board, _)) => {
            let how = if epic.state == "open" { Follow::Open } else { Follow::Closed(done_note(&epic.state_group)) };
            let changed = app.db.follow_epic(&board.id, &epic.title, how, now)?;
            (board, changed)
        }
        // A closed epic with no board gets none.
        None if epic.state != "open" => return Ok(()),
        None => {
            let goal = read_goal(app, c, epic).await;
            let key = format!("{}-{}", c.identifier, epic.number);
            let url = trackers::item_url(&c.workspace_slug, &c.identifier, epic.number);
            let new = NewEpicBoard {
                project: &c.project_id,
                title: &epic.title,
                goal: &goal,
                source_item: &epic.id,
                key: &key,
                url: &url,
                connection: &c.name,
            };
            let Some(board) = app.db.create_epic_board(&new, now, arugula_control_wire::boards::BOARDS_MAX)? else {
                warn!(connection = %c.id, "boards: a project has no room for an epic's board");
                return Ok(());
            };
            (board, true)
        }
    };
    let wanted: Vec<Want> = app
        .db
        .children_of(&c.id, &epic.source_item_id)?
        .into_iter()
        .map(|n| Want { id: n.id, done: n.state != "open", updated_at: n.updated_at })
        .collect();
    if let Some(rows) = app.db.sync_epic_rows(&board.id, &wanted, now, ROWS_MAX)? {
        changed |= rows.changed;
    }
    if changed {
        super::changed(app, &board.project, &board.id);
    }
    Ok(())
}

/// The first paragraph of the epic's description, read from the tracker
/// once; empty if it can't be read (the board is made all the same, and its
/// goal is a person's to write). Never logged.
async fn read_goal(app: &Platform, c: &Connection, epic: &Node) -> String {
    let read = async {
        let master = app.secret_key.as_deref()?;
        let key = keys::open(&app.db, master, &c.project_id, &key_name(&c.id)).ok()??;
        let provider: std::sync::Arc<dyn TrackerProvider> = app.trackers.plane(&key);
        match provider.get(&c.workspace_slug, &c.source_project_id, &epic.source_item_id).await {
            Ok(item) => Some(goal_of(&item.description.unwrap_or_default())),
            Err(TrackerError::NotFound) => None,
            Err(e) => {
                warn!(connection = %c.id, error = %e, "boards: couldn't read an epic's description");
                None
            }
        }
    };
    read.await.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_goal_is_the_first_line_in_plain_text_and_at_most_the_limit() {
        assert_eq!(goal_of("\n\nShip it   on time.\n\nNot this."), "Ship it on time.");
        assert_eq!(
            goal_of(
                "Checks whether Arugula's mirror sees a Plane epic and its children.\nSecond paragraph, which \
                 shouldn't become the goal."
            ),
            "Checks whether Arugula's mirror sees a Plane epic and its children."
        );
        assert_eq!(goal_of(""), "");
        assert_eq!(goal_of(&"x".repeat(GOAL_MAX + 10)).chars().count(), GOAL_MAX);
    }
}
