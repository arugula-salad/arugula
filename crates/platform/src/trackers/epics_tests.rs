//! Plane epics as boards (Arugula #483), through the platform's own routes
//! against `FakeTracker`: an open epic has a board whose rows are its
//! children, in the lanes their states give, and the board follows the epic.

use std::{sync::Arc, time::Duration};

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    Item, connect,
    connect_tests::{KEY, backfilled, connect_as, serve},
    fake::{FakeTracker, item},
};
use crate::{
    Platform,
    boards::{FLAG, epics},
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params, trackers as rows},
};

struct World {
    app: Arc<Platform>,
    base: String,
    fake: FakeTracker,
    team: String,
    conn: String,
    mirror: i64,
}

/// A connected, backfilled `ARUGU` with no epics, and bob who may write.
async fn world() -> World {
    let hub = Arc::new(TestHub::default());
    let (fake, app, base) = serve(&hub).await;
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    hub.flag_named("team:t1", FLAG, true);
    let team = crate::projects::team_project(&app, "t1").await.unwrap().unwrap();
    let (s, made) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let conn = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &team, &conn).await;
    let mirror = app.db.tracker_connection(&team, &conn).unwrap().unwrap().mirror_id;
    World { app, base, fake, team, conn, mirror }
}

fn epic(n: u64, title: &str, group: &str, at: u64) -> Item {
    let mut e = item(n, title, group, at);
    e.item_type = Some("Epic".into());
    e.description = Some("Ship the launch.\n\nThe second paragraph is left out.".into());
    e
}

fn child(n: u64, parent: u64, group: &str, at: u64) -> Item {
    let mut c = item(n, &format!("Child {n}"), group, at);
    c.parent_id = Some(format!("item-{parent}"));
    c
}

impl World {
    /// Plane has `all`; a page of `page` arrives (the mirror writes it whole, then the boards follow).
    async fn arrive(&self, all: Vec<Item>, page: &[Item]) -> Vec<String> {
        self.fake.set_items(all);
        let changed = connect::ingest_page(&self.app, &self.conn, self.mirror, None, page, 10_000).unwrap().unwrap();
        epics::from_epics(&self.app, &self.conn, &changed).await;
        changed
    }

    fn fi(&self, n: u64) -> String {
        self.app
            .db
            .c()
            .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 AND number = ?2", params![self.mirror, n], |r| {
                r.get(0)
            })
            .unwrap()
    }

    async fn boards(&self) -> Vec<Value> {
        let (s, v) = call(&self.base, "carol", "GET", &format!("/api/projects/{}/boards", self.team), None).await;
        assert_eq!(s, 200, "{v}");
        v["boards"].as_array().unwrap().clone()
    }

    async fn board(&self, id: &str) -> Value {
        let (s, v) = call(&self.base, "carol", "GET", &format!("/api/boards/{id}"), None).await;
        assert_eq!(s, 200, "{v}");
        v
    }

    /// The only board.
    async fn only(&self) -> String {
        let boards = self.boards().await;
        assert_eq!(boards.len(), 1, "{boards:?}");
        boards[0]["id"].as_str().unwrap().to_owned()
    }

    /// The board's rows as (lane, number of the Plane item), lane by lane.
    async fn rows(&self, id: &str) -> Vec<(String, u64)> {
        let v = self.board(id).await;
        let mut out = vec![];
        for z in v["zones"].as_array().unwrap() {
            for r in z["rows"].as_array().unwrap() {
                out.push((z["title"].as_str().unwrap().to_owned(), r["live"]["number"].as_u64().unwrap_or(0)));
            }
        }
        out
    }
}

fn lane(name: &str, n: u64) -> (String, u64) {
    (name.to_owned(), n)
}

/// An epic with three children makes one board with three rows in the lanes
/// their states give, though a child came before its epic in the page; the
/// board is made by the connection and says where from, takes the first
/// paragraph of the description as its goal, and keeps the epic out of
/// Triage. Running it again adds nothing; a fourth child adds a row.
#[tokio::test]
async fn an_epic_makes_a_board_with_its_children_as_rows() {
    let w = world().await;
    let all = vec![
        child(12, 10, "started", 2_000),
        epic(10, "Launch", "started", 3_000),
        child(11, 10, "backlog", 2_000),
        child(13, 10, "completed", 2_000),
    ];
    // The page has a child first, then its epic.
    w.arrive(all.clone(), &all).await;
    let id = w.only().await;
    let b = &w.boards().await[0];
    assert_eq!(
        (b["title"].as_str(), b["goal"].as_str(), b["created_by"].as_str()),
        (Some("Launch"), Some("Ship the launch."), Some(""))
    );
    assert_eq!(b["source"]["key"], "ARUGU-10");
    assert_eq!(b["source"]["url"], "https://app.plane.so/acme/browse/ARUGU-10/");
    assert_eq!(b["source"]["provider"], "plane");
    assert!(b.get("created_by_agent").is_none());
    let rows = w.rows(&id).await;
    assert_eq!(rows, [lane("Next", 11), lane("Doing", 12), lane("Done", 13)]);

    // Again: nothing is added, and the rows are the same rows.
    let before = w.board(&id).await;
    w.arrive(all.clone(), &all).await;
    epics::first_pass(&w.app, &w.conn).await;
    assert_eq!(w.boards().await.len(), 1);
    assert_eq!(w.board(&id).await["zones"], before["zones"]);

    // The epic isn't in Triage.
    let (s, v) = call(&w.base, "carol", "GET", &format!("/api/projects/{}/work?untriaged=true", w.team), None).await;
    assert_eq!(s, 200, "{v}");
    let numbers: Vec<u64> = v["items"].as_array().unwrap().iter().filter_map(|i| i["number"].as_u64()).collect();
    assert!(!numbers.contains(&10), "{numbers:?}");

    // A fourth child.
    let mut more = all.clone();
    more.push(child(14, 10, "started", 4_000));
    w.arrive(more.clone(), &more[4..]).await;
    assert_eq!(w.rows(&id).await.len(), 4);
}

/// A child can arrive in an earlier page than its epic (the backfill reads
/// pages by recency): the board is made when the epic comes, with the
/// child already in the mirror.
#[tokio::test]
async fn a_child_in_an_earlier_page_than_its_epic_is_found_when_the_epic_arrives() {
    let w = world().await;
    let all = vec![epic(10, "Launch", "started", 3_000), child(11, 10, "backlog", 2_000)];
    w.arrive(all.clone(), &all[1..]).await;
    assert!(w.boards().await.is_empty());
    w.arrive(all.clone(), &all[..1]).await;
    let id = w.only().await;
    assert_eq!(w.rows(&id).await, [lane("Next", 11)]);
}

/// A child moved from one epic to another leaves the first board and joins
/// the second, though the mirror only knows its new parent; a deleted
/// child leaves its board.
#[tokio::test]
async fn a_moved_child_changes_boards_and_a_deleted_one_leaves() {
    let w = world().await;
    let all = vec![
        epic(10, "Launch", "started", 3_000),
        epic(20, "Docs", "started", 3_000),
        child(11, 10, "backlog", 2_000),
        child(12, 10, "started", 2_000),
    ];
    w.arrive(all.clone(), &all).await;
    let boards = w.boards().await;
    assert_eq!(boards.len(), 2);
    let id_of = |title: &str| boards.iter().find(|b| b["title"] == title).unwrap()["id"].as_str().unwrap().to_owned();
    let (launch, docs) = (id_of("Launch"), id_of("Docs"));
    assert_eq!(w.rows(&launch).await.len(), 2);
    assert!(w.rows(&docs).await.is_empty());

    // Child 12 moves to epic 20 in Plane: only it arrives.
    let moved = child(12, 20, "started", 5_000);
    let all = vec![all[0].clone(), all[1].clone(), all[2].clone(), moved.clone()];
    w.arrive(all.clone(), &[moved]).await;
    assert_eq!(w.rows(&launch).await, [lane("Next", 11)]);
    assert_eq!(w.rows(&docs).await, [lane("Doing", 12)]);

    // Child 11 is deleted in Plane.
    let gone = w.fi(11);
    w.app.db.tracker_mirror(&w.conn, None, |tx| rows::delete_item(tx, &w.conn, "item-11")).unwrap();
    epics::from_epics(&w.app, &w.conn, &[gone]).await;
    assert!(w.rows(&launch).await.is_empty());
}

/// The board's title follows the epic's, completing the epic closes the
/// board with a note and reopening it opens the board; a board a person
/// closed is not opened by the sync; a deleted epic closes its board; and a
/// completed epic with no board gets none.
#[tokio::test]
async fn the_board_follows_its_epic_through_a_rename_a_close_and_a_reopen() {
    let w = world().await;
    let all = vec![epic(10, "Launch", "started", 3_000), child(11, 10, "backlog", 2_000)];
    w.arrive(all.clone(), &all).await;
    let id = w.only().await;

    let renamed = epic(10, "Launch day", "started", 4_000);
    w.arrive(vec![renamed.clone(), all[1].clone()], std::slice::from_ref(&renamed)).await;
    assert_eq!(w.boards().await[0]["title"], "Launch day");

    let done = epic(10, "Launch day", "completed", 5_000);
    w.arrive(vec![done.clone(), all[1].clone()], &[done]).await;
    let b = &w.boards().await[0];
    assert_eq!((b["done_note"].as_str(), b["closed_at"].is_u64()), (Some("The epic was completed in Plane"), true));

    let back = epic(10, "Launch day", "started", 6_000);
    w.arrive(vec![back.clone(), all[1].clone()], std::slice::from_ref(&back)).await;
    assert!(w.boards().await[0]["closed_at"].is_null());

    // A person closes it while the epic is open: the sync leaves it closed.
    let (s, v) =
        call(&w.base, "bob", "POST", &format!("/api/boards/{id}/close"), Some(json!({ "done_note": "Parked" }))).await;
    assert_eq!(s, 200, "{v}");
    let again = epic(10, "Launch day", "started", 7_000);
    w.arrive(vec![again.clone(), all[1].clone()], &[again]).await;
    assert!(w.boards().await[0]["closed_at"].is_u64());

    // Cancelled says so; deleted closes (a board already closed keeps its first close, so reopen it first).
    let (s, _) = call(&w.base, "bob", "POST", &format!("/api/boards/{id}/reopen"), None).await;
    assert_eq!(s, 200);
    let cancelled = epic(10, "Launch day", "cancelled", 8_000);
    w.arrive(vec![cancelled.clone(), all[1].clone()], &[cancelled]).await;
    assert_eq!(w.boards().await[0]["done_note"], "The epic was cancelled in Plane");
    let (s, _) = call(&w.base, "bob", "POST", &format!("/api/boards/{id}/reopen"), None).await;
    assert_eq!(s, 200);
    let fi = w.fi(10);
    w.app.db.tracker_mirror(&w.conn, None, |tx| rows::delete_item(tx, &w.conn, "item-10")).unwrap();
    epics::from_epics(&w.app, &w.conn, &[fi]).await;
    assert_eq!(w.boards().await[0]["done_note"], "The epic was deleted in Plane");

    // A completed epic that never had a board gets none.
    let old = epic(30, "Old", "completed", 9_000);
    w.arrive(vec![old.clone()], &[old]).await;
    assert_eq!(w.boards().await.len(), 1);
}

/// A row a person added is never claimed, duplicated or removed by the sync,
/// even when its child leaves the epic; and removing a row the sync added
/// is refused, with the reason.
#[tokio::test]
async fn a_persons_row_is_theirs_and_a_synced_row_cannot_be_removed_by_hand() {
    let w = world().await;
    let all = vec![epic(10, "Launch", "started", 3_000), child(11, 10, "backlog", 2_000)];
    w.arrive(all.clone(), &all).await;
    let id = w.only().await;

    // bob puts child 12 on the board before the sync has seen it.
    let c12 = child(12, 10, "started", 2_500);
    let c13 = child(13, 10, "started", 2_600);
    let all = vec![all[0].clone(), all[1].clone(), c12.clone(), c13.clone()];
    w.fake.set_items(all.clone());
    let changed = connect::ingest_page(&w.app, &w.conn, w.mirror, None, &[c12.clone(), c13], 10_000).unwrap().unwrap();
    let ptr = json!({ "kind": "forge_item", "id": w.fi(12) });
    let (s, v) = call(
        &w.base,
        "bob",
        "PUT",
        &format!("/api/boards/{id}/entries"),
        Some(json!({ "entry": "row", "note": "mine", "pointer": ptr })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    epics::from_epics(&w.app, &w.conn, &changed).await;
    let rows = w.rows(&id).await;
    assert_eq!(rows.iter().filter(|r| r.1 == 12).count(), 1, "{rows:?}: one row for the piece of work");
    assert!(rows.iter().any(|r| r.1 == 13), "{rows:?}: and the sync went on to add the next child");

    // Child 12 leaves for a project of no epic: bob's row stays.
    let loose = item(12, "Child 12", "started", 6_000);
    w.arrive(vec![all[0].clone(), all[1].clone(), loose.clone(), all[3].clone()], &[loose]).await;
    assert!(w.rows(&id).await.iter().any(|r| r.1 == 12), "a person's row stays");

    // The synced row of child 11 can't be removed by hand.
    let v = w.board(&id).await;
    let row = v["zones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|z| z["rows"].as_array().unwrap())
        .find(|r| r["live"]["number"] == 11)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (s, v) = call(&w.base, "bob", "DELETE", &format!("/api/boards/{id}/entries/{row}"), None).await;
    assert_eq!(s, 400, "{v}");
    assert!(v.to_string().contains("ARUGU-11 is a child of ARUGU-10 in Plane: change its parent there"), "{v}");
    assert!(w.rows(&id).await.iter().any(|r| r.1 == 11));
}

/// With the switch off the boards stay and nothing follows; turned on, the
/// epics the switch missed get their boards.
#[tokio::test]
async fn the_switch_stops_the_sync_and_turning_it_on_catches_up() {
    let w = world().await;
    let patch = |on: bool| {
        let (base, team, conn) = (w.base.clone(), w.team.clone(), w.conn.clone());
        async move {
            let (s, v) = call(
                &base,
                "bob",
                "PATCH",
                &format!("/api/projects/{team}/trackers/{conn}"),
                Some(json!({ "epics_as_boards": on })),
            )
            .await;
            assert_eq!(s, 200, "{v}");
            assert_eq!(v["epics_as_boards"], on);
        }
    };
    let all = vec![epic(10, "Launch", "started", 3_000), child(11, 10, "backlog", 2_000)];
    w.arrive(all.clone(), &all).await;
    let id = w.only().await;

    patch(false).await;
    let renamed = epic(10, "Launch day", "started", 4_000);
    let two =
        vec![renamed.clone(), all[1].clone(), epic(20, "Docs", "started", 4_000), child(21, 20, "started", 4_000)];
    w.arrive(two.clone(), &two).await;
    let boards = w.boards().await;
    assert_eq!(boards.len(), 1, "no board for the new epic");
    assert_eq!(boards[0]["title"], "Launch", "the old board keeps its title");
    assert_eq!(w.rows(&id).await.len(), 1);

    patch(true).await;
    for _ in 0..200 {
        if w.boards().await.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let boards = w.boards().await;
    assert_eq!(boards.len(), 2);
    assert!(boards.iter().any(|b| b["title"] == "Launch day"), "{boards:?}");
}

/// A board too full for all the children takes the open ones and the newest
/// done ones, leaves the rest out and says how many, and a person's rows
/// count against the room.
#[tokio::test]
async fn a_full_board_leaves_out_the_oldest_done_children_and_counts_them() {
    use crate::db::boards_epics::Want;

    let w = world().await;
    let all = vec![
        epic(10, "Launch", "started", 3_000),
        child(11, 10, "completed", 1_000),
        child(12, 10, "completed", 2_000),
        child(13, 10, "started", 1_500),
        child(14, 10, "backlog", 1_500),
    ];
    w.arrive(all.clone(), &all).await;
    let id = w.only().await;
    let want = |n: u64, done: bool, at: u64| Want { id: w.fi(n), done, updated_at: at };
    let wanted = [want(11, true, 1_000), want(12, true, 2_000), want(13, false, 1_500), want(14, false, 1_500)];
    let synced = w.app.db.sync_epic_rows(&id, &wanted, 20_000, 3).unwrap().unwrap();
    assert_eq!((synced.removed, synced.left_out), (1, 1), "{synced:?}");
    let rows = w.rows(&id).await;
    assert_eq!(rows, [lane("Next", 14), lane("Doing", 13), lane("Done", 12)], "the oldest done child is out");
    assert_eq!(w.boards().await[0]["source"]["left_out"], 1);
}
