//! A merged PR moves the Plane items it names to Done (Arugula #437), through
//! the mirror's own webhook, in the world of `closes_tests` with a fake Plane
//! connected to the project: the item's PR merges and, once the job queue
//! has run, the fake has been sent one state change to its completed state,
//! the mirror says so and the board's row is Done. `part of`, a PR closed
//! unmerged, an item finished already (even one the mirror hasn't heard of)
//! and a key no connection holds send nothing; a redelivery sends nothing
//! more; and a Plane that is down once is tried again, while the native
//! closes and the webhook go on regardless.

use std::sync::Arc;

use serde_json::json;

use super::{
    closes::merged,
    closes_tests::{BOB, deliver, mirror_bodies, pr, pr_id},
    lanes_tests::{add, check, fi, native, on, read},
    links_tests::serve_with,
    viewer_tests::world,
};
use crate::{
    Platform,
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{
        Sql as _,
        jobs::{JobRow, RETRY_MS},
        sql::params,
    },
    jobs::{Clock, Worker},
    trackers::{
        connect_tests::{KEY, backfilled, connect_as, plane},
        done::Moves,
        fake::{FakeTracker, item},
    },
};

struct World {
    base: String,
    app: Arc<Platform>,
    fake: FakeTracker,
    team: String,
    board: String,
    connection: String,
}

/// The `closes_tests` world (acme/site is repo 88, linked to team t1) with a
/// board of bob's and the fake Plane's `ARUGU` connected to the team.
async fn start() -> World {
    let hub = Arc::new(TestHub::default());
    let mut fake = None;
    let (gh, app, base) = serve_with(&hub, |p| fake = Some(plane(p))).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    let (s, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Lanes" }))).await;
    assert_eq!(s, 200, "{b}");
    let board = b["board"]["id"].as_str().unwrap().to_owned();
    let (s, made) = connect_as(&base, "bob", &team, KEY, "ARUGU").await;
    assert_eq!(s, 200, "{made}");
    let connection = made["id"].as_str().unwrap().to_owned();
    backfilled(&app, &team, &connection).await;
    mirror_bodies(&app);
    World { base, app, fake: fake.unwrap(), team, board, connection }
}

impl World {
    /// The mirror's row for `ARUGU-n`.
    fn row(&self, n: u64) -> String {
        self.app
            .db
            .c()
            .query_row(
                "SELECT id FROM forge_items WHERE connection_id = ?1 AND number = ?2",
                params![self.connection, n],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn group(&self, n: u64) -> Option<String> {
        self.app.db.tracker_item_group(&self.connection, &format!("item-{n}")).unwrap()
    }

    /// The jobs that move a Plane item, by id.
    fn jobs(&self) -> Vec<JobRow> {
        let ids: Vec<String> = self
            .app
            .db
            .c()
            .query_map("SELECT id FROM platform_jobs WHERE kind = 'plane_done' ORDER BY id", params![], |r| r.get(0))
            .unwrap();
        ids.iter().map(|id| self.app.db.platform_job(id).unwrap().unwrap()).collect()
    }

    /// A PR as the webhook sees it, merged by bob (or not).
    async fn merge(&self, number: u64, title: &str, merged: bool) {
        let p = pr(number, title, "");
        deliver(&self.app, "opened", &p, 9, false, BOB).await;
        deliver(&self.app, "closed", &p, 10, merged, BOB).await;
    }

    /// The queue's worker at `now`, until nothing is due: the steps it took.
    async fn run(&self, now: u64) -> usize {
        let worker = Worker::new().with(Moves { app: self.app.clone() }).clock(Clock::new(move || now));
        let mut steps = 0;
        while worker.tick(&self.app.db).await.unwrap() {
            steps += 1;
        }
        steps
    }
}

fn now() -> u64 {
    arugula_e2e::now_ms()
}

/// A PR that names `ARUGU-2` in its title is merged with CI green: nothing
/// is sent by the webhook itself, the job then sends one state change to
/// Done, the mirror and the board's row follow it, and the connection's line
/// is kept. Delivered again and read again by the poller's pass, it sends
/// nothing more.
#[tokio::test]
async fn a_merged_pr_moves_the_plane_item_to_done() {
    let w = start().await;
    let second = w.row(2);
    let v = add(&w.base, &w.board, fi(&second)).await;
    assert_ne!(on(&v, &second).0, "lane_done");

    let p = pr(70, "ARUGU-2: plumbing", "");
    deliver(&w.app, "opened", &p, 9, false, BOB).await;
    check(&w.app, 88, "h70", "build", "completed", Some("success"));
    assert_eq!(on(&read(&w.base, "bob", &w.board).await, &second).0, "lane_review");
    deliver(&w.app, "closed", &p, 10, true, BOB).await;
    assert!(w.fake.sent().is_empty(), "the webhook sends nothing itself");
    assert_eq!(w.jobs().len(), 1);

    assert_eq!(w.run(now()).await, 1);
    let sent = w.fake.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].kind.as_str(), sent[0].item.as_deref(), sent[0].state.as_deref(), sent[0].html.as_deref()),
        ("state", Some("item-2"), Some("st-done"), None),
        "a state change alone: no comment, no body, no footer"
    );
    assert_eq!(w.group(2).as_deref(), Some("completed"));
    let v = read(&w.base, "bob", &w.board).await;
    assert_eq!(on(&v, &second).0, "lane_done", "{v}");
    assert_eq!(on(&v, &second).1["proof"], json!({ "by": "ci", "pointer": fi(&pr_id(&w.app, 70)) }), "{v}");
    let job = &w.jobs()[0];
    assert_eq!(job.state, "done");
    assert_eq!(
        job.checkpoint.as_ref().unwrap()["moved"],
        "moved ARUGU-2 to Done: merged acme/site#70",
        "{job:?}"
    );
    assert_eq!(job.checkpoint.as_ref().unwrap()["by"], "bob", "bob is in the project");

    deliver(&w.app, "closed", &p, 10, true, BOB).await;
    merged(&w.app, 88, &[70]).await;
    assert_eq!(w.jobs().len(), 1, "one job to a PR and item");
    assert_eq!(w.run(now()).await, 0);
    assert_eq!(w.fake.sent().len(), 1, "nothing sent twice");
}

/// `part of ARUGU-1`, `refs` and `see`, a PR closed unmerged and a key no
/// connection holds send nothing, and queue nothing. Named plainly beside a
/// soft mention, an item moves. An item finished already is left alone, and
/// so is one the mirror thinks open that Plane already finished.
#[tokio::test]
async fn only_a_plain_name_on_a_merge_moves_an_open_item() {
    let w = start().await;
    for (n, title) in [(71, "Part of ARUGU-1: x"), (72, "Refs ARUGU-1, see ARUGU-2"), (73, "ZZZ-1 and ARUGU-1")] {
        w.merge(n, title, n != 73).await;
    }
    w.merge(74, "ARUGU-1: closed unmerged", false).await;
    assert!(w.jobs().is_empty(), "{:?}", w.jobs());
    assert_eq!(w.run(now()).await, 0);
    assert!(w.fake.sent().is_empty());
    assert_eq!(w.group(1).as_deref(), Some("backlog"));

    // A finished item (ARUGU-3) is left alone; so is one only Plane knows is done.
    w.merge(75, "ARUGU-3: done twice", true).await;
    let mut done = item(2, "Second", "completed", 9_000);
    done.state_name = "Done".into();
    w.fake.set_items(vec![item(1, "First", "backlog", 3_000), done, item(3, "Third", "completed", 1_000)]);
    assert_eq!(w.group(2).as_deref(), Some("started"), "the mirror hasn't heard");
    w.merge(76, "ARUGU-2: plumbing", true).await;
    assert_eq!(w.jobs().len(), 2);
    assert_eq!(w.run(now()).await, 2);
    assert!(w.fake.sent().is_empty(), "{:?}", w.fake.sent());
    assert_eq!(w.group(2).as_deref(), Some("completed"), "the mirror is mended");

    // Plain beside soft: moved.
    w.merge(77, "Part of ARUGU-1, fixes ARUGU-1", true).await;
    assert_eq!(w.run(now()).await, 1);
    assert_eq!(w.fake.sent().len(), 1);
    assert_eq!(w.group(1).as_deref(), Some("completed"));
}

/// Plane is down for the first state change: the merge still closes the native
/// issue the PR names and the webhook is done, the job fails and waits, and
/// once its back-off is over the move lands, once.
#[tokio::test]
async fn a_plane_that_is_down_is_tried_again() {
    let w = start().await;
    let (issue, n) = native(&w.base, &w.team, "Native").await;
    w.fake.fail_state_changes(1);
    w.merge(80, &format!("Arugula #{n} and ARUGU-1"), true).await;
    let (_, got) = call(&w.base, "bob", "GET", &format!("/api/issues/{issue}"), None).await;
    assert_eq!(got["state"], "closed", "the native close doesn't wait for Plane");

    let t = now();
    assert_eq!(w.run(t).await, 1);
    let job = &w.jobs()[0];
    assert_eq!((job.state.as_str(), job.attempts), ("pending", 1), "{job:?}");
    assert!(job.error.as_deref().unwrap().contains("the fake is down"), "{job:?}");
    assert!(w.fake.sent().is_empty());
    assert_eq!(w.run(t).await, 0, "not before its back-off");

    assert_eq!(w.run(t + RETRY_MS + 1_000).await, 1);
    assert_eq!(w.fake.sent().len(), 1, "{:?}", w.fake.sent());
    assert_eq!(w.group(1).as_deref(), Some("completed"));
    assert_eq!(w.jobs()[0].state, "done");
    assert_eq!(w.run(t + 2 * RETRY_MS).await, 0);
    assert_eq!(w.fake.sent().len(), 1);
}
