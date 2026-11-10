//! The launch switches for hosted boxes (#531), against a fake provider
//! and the test hub's standings: new boxes off or waitlisted, the caps on
//! live workspaces and box-hours, and a suspended maker's boxes stopped.
//! Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use arugula_e2e::now_ms;
use serde_json::Value;

use super::*;
use crate::{
    chat_tests::{TestHub, call},
    db::Db,
    hosted::{
        EnrollBy, Spec,
        hold::{Holds, tick_at},
        tests::{drive, make, serve},
        wake,
    },
};

fn standing(account: &str, set: impl FnOnce(&mut Standing)) -> Standing {
    let mut s = TestHub::standing(account);
    set(&mut s);
    s
}

/// A refusal's status, reason and sentence.
fn refused(r: &(u16, Value)) -> (u16, &str, &str) {
    (r.0, r.1["reason"].as_str().unwrap_or_default(), r.1["error"].as_str().unwrap_or_default())
}

/// `off` refuses every new box and rebuild, saying the ones there keep
/// going; `waitlist` refuses whoever isn't admitted (or on the platform's
/// own list) and puts them on the waitlist; an admission lets them in, and
/// `open` lets anyone. The shell offers hosted workspaces to whom they're
/// open. When the hub can't say, nothing is made.
#[tokio::test]
async fn new_boxes_follow_the_switches() {
    let (hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    let carols = app.db.team_project("t2", "t2", 0).unwrap();
    let hosted = |who: &'static str| {
        let base = base.clone();
        async move { call(&base, who, "GET", "/api/projects", None).await.1["hosted"].clone() }
    };

    // Waitlisted (the test hub's default): carol isn't admitted, and goes
    // on the waitlist; alice is on the platform's list.
    let r = make(&base, "carol", &carols, "dev").await;
    let (status, reason, say) = refused(&r);
    assert_eq!((status, reason), (403, WAITLIST), "{}", r.1);
    assert!(say.contains("You're on the waitlist now"), "{say}");
    assert_eq!(hub.waiting(), ["carol"]);
    assert_eq!(hosted("carol").await, false);
    assert_eq!(make(&base, "alice", &project, "a1").await.0, 201);
    assert_eq!(hub.waiting(), ["carol"], "alice wasn't asked to wait");

    // Admitted.
    hub.set_standing(standing("carol", |s| s.admitted = true));
    assert_eq!(hosted("carol").await, true);
    let (status, w) = make(&base, "carol", &carols, "dev").await;
    assert_eq!(status, 201, "{w}");

    // Off: nobody, admitted or listed, for a new box or a rebuild.
    for who in ["alice", "carol"] {
        hub.set_standing(standing(who, |s| {
            s.hosted = "off".into();
            s.admitted = true;
        }));
    }
    let r = make(&base, "alice", &project, "a2").await;
    let (status, reason, say) = refused(&r);
    assert_eq!((status, reason), (503, OFF));
    assert!(say.starts_with("New cloud workspaces are turned off for now. The ones there are keep running"), "{say}");
    let path = format!("/api/projects/{carols}/workspaces/dev/rebuild");
    let r = call(&base, "carol", "POST", &path, None).await;
    assert_eq!(refused(&r).1, OFF, "a rebuild makes a box");
    assert_eq!(hosted("carol").await, true, "what's there is still hers");
    let path = format!("/api/projects/{carols}/workspaces/dev");
    assert_eq!(call(&base, "carol", "GET", &path, None).await.0, 200, "and opens");

    // Open: anyone, admitted or not.
    hub.set_standing(standing("carol", |s| s.hosted = "open".into()));
    assert_eq!(make(&base, "carol", &carols, "dev2").await.0, 201);

    // A suspended account: nothing.
    hub.set_standing(standing("carol", |s| s.suspended = true));
    let r = make(&base, "carol", &carols, "dev3").await;
    assert_eq!(refused(&r).1, SUSPENDED);
    assert_eq!(hosted("carol").await, false);

    // The hub can't say: nothing made, and only the list opens them.
    hub.standings_down(true);
    let r = make(&base, "carol", &carols, "dev4").await;
    assert_eq!(r.0, 503, "{}", r.1);
    assert!(refused(&r).2.contains("try again in a minute"));
    assert_eq!(hosted("alice").await, true);
    assert_eq!(hosted("carol").await, false);
}

/// An account may have so many live workspaces it made, in every project
/// together: past it, a refusal that says to delete one. A deleted one
/// makes room.
#[tokio::test]
async fn an_account_has_its_cap_of_live_workspaces() {
    let (hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.set_standing(standing("alice", |s| s.boxes = 2));
    assert_eq!(make(&base, "alice", &project, "one").await.0, 201);
    assert_eq!(make(&base, "alice", "personal", "two").await.0, 201);
    let r = make(&base, "alice", &project, "three").await;
    let (status, reason, say) = refused(&r);
    assert_eq!((status, reason), (409, BOX_CAP), "{}", r.1);
    assert_eq!(
        say,
        "You've made 2 cloud workspaces that are still there, the most one account may have for now. Delete one \
         to make another."
    );
    assert_eq!(make(&base, "bob", &project, "bobs").await.0, 201, "bob's cap is his own");
    let path = format!("/api/projects/{project}/workspaces/one");
    assert_eq!(call(&base, "alice", "DELETE", &path, None).await.0, 200);
    drive(&app, now_ms()).await;
    assert_eq!(make(&base, "alice", &project, "three").await.0, 201);
}

/// The hours the boxes of an account's workspaces ran, in a window:
/// deleted workspaces' too, an open interval to the window's end, and not
/// another maker's.
#[test]
fn box_hours_are_counted_by_maker() {
    use crate::db::sql::{Sql as _, params};
    let db = Db::test();
    let spec = Spec::default();
    for (id, by) in [("hw_a", "alice"), ("hw_b", "alice"), ("hw_c", "bob")] {
        db.add_hosted_workspace(id, "prj", id, &spec, by, 10, 0).unwrap();
    }
    let h = 3_600_000;
    for (w, s, e) in
        [("hw_a", 0, Some(2 * h)), ("hw_a", 5 * h, Some(6 * h)), ("hw_b", 9 * h, None), ("hw_c", 0, Some(h))]
    {
        db.c()
            .execute(
                "INSERT INTO hosted_usage (workspace_id, project_id, sandbox_id, started_at, ended_at)
                 VALUES (?1, 'prj', ?2, ?3, ?4)",
                params![w, format!("sbx-{w}-{s}"), s, e],
            )
            .unwrap();
    }
    db.c().execute("UPDATE hosted_workspaces SET deleted_at = 1 WHERE id = 'hw_a'", params![]).unwrap();
    assert_eq!(db.hosted_ms_made_by("alice", h, 10 * h).unwrap(), h + h + h, "1 of the first, the second, 1 open");
    assert_eq!(db.hosted_ms_made_by("bob", 0, 10 * h).unwrap(), h);
    assert_eq!(db.hosted_ms_made_by("carol", 0, 10 * h).unwrap(), 0);
}

/// Past its box-hours this month, an account makes no box, and its
/// workspaces' boxes don't wake, by an open or the hub's wake on connect,
/// whoever opens them; each says when the hours start again. Its running
/// boxes are paused between turns.
#[tokio::test]
async fn past_its_hours_nothing_starts() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let now = now_ms();
    drive(&app, now).await;
    hub.enroll(&id, "m1");
    drive(&app, now + 3_000).await;
    fake.calls();

    // 0 hours left (whatever day of the month this runs on).
    hub.set_standing(standing("alice", |s| s.box_hours = 0));
    let r = make(&base, "alice", &project, "dev2").await;
    let (status, reason, say) = refused(&r);
    assert_eq!((status, reason), (429, BOX_HOURS), "{}", r.1);
    assert!(say.starts_with("Your cloud workspaces have used this account's 0 box-hours for this month."), "{say}");
    assert!(say.contains("They start again on 1 "), "{say}");

    // Running and busy: the sweep asks a pause, which waits for the turn.
    hub.say_busy("m1", Some(true));
    assert!(sweep(&app, now_ms()).await.unwrap().is_empty(), "not stopped: paused between turns");
    drive(&app, now + 10_000).await;
    assert!(fake.calls().is_empty(), "not mid-turn");
    hub.say_busy("m1", Some(false));
    sweep(&app, now_ms()).await.unwrap();
    drive(&app, now + 20_000).await;
    assert_eq!(fake.calls(), ["pause sbx0"]);

    // Paused: bob's open doesn't wake it, nor does the hub's wake.
    let path = format!("/api/projects/{project}/workspaces/{id}");
    let r = call(&base, "bob", "GET", &path, None).await;
    assert_eq!(refused(&r).1, BOX_HOURS, "{}", r.1);
    assert!(refused(&r).2.starts_with("This cloud workspace can't start: the account that made it has used its 0"));
    let a = wake::wake(&app, &project, &id).await.unwrap();
    assert!(
        matches!(&a, arugula_control_wire::hosted::WakeAnswer::Failed { code: arugula_control_wire::hosted::WakeCode::Refused, why } if why.contains("box-hours")),
        "{a:?}"
    );
    assert!(fake.calls().is_empty());

    // More hours: it wakes.
    hub.set_standing(standing("alice", |s| s.box_hours = 60));
    assert_eq!(call(&base, "bob", "GET", &path, None).await.0, 200);
    drive(&app, now + 30_000).await;
    assert_eq!(fake.calls(), ["resume sbx0"]);
}

/// A suspended maker's box is paused at once, mid-turn too; the hold
/// neither holds nor wakes it, and nobody's open does. Another maker's busy
/// box is left alone. Lifted, it wakes when opened.
#[tokio::test]
async fn a_suspended_makers_boxes_stop() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let now = now_ms();
    let mut ids = vec![];
    for (who, name) in [("alice", "alices"), ("bob", "bobs")] {
        let (_, w) = make(&base, who, &project, name).await;
        ids.push(w["id"].as_str().unwrap().to_owned());
    }
    drive(&app, now).await;
    hub.enroll(&ids[0], "m-alice");
    hub.enroll(&ids[1], "m-bob");
    drive(&app, now + 3_000).await;
    hub.say_busy("m-alice", Some(true));
    hub.say_busy("m-bob", Some(true));
    fake.calls();

    hub.set_standing(standing("alice", |s| s.suspended = true));
    let stopped = sweep(&app, now_ms()).await.unwrap();
    assert_eq!(stopped.into_iter().collect::<Vec<_>>(), [ids[0].clone()]);
    drive(&app, now + 10_000).await;
    let alices = app.db.hosted_workspace(&project, &ids[0]).unwrap().unwrap();
    assert_eq!(alices.state, "paused", "mid-turn, all the same");
    assert_eq!(app.db.hosted_workspace(&project, &ids[1]).unwrap().unwrap().state, "ready", "bob's is his");
    let paused: Vec<String> = fake.calls().into_iter().filter(|c| c.starts_with("pause")).collect();
    assert_eq!(paused.len(), 1, "{paused:?}");

    // The hold, still told it's busy, neither holds nor wakes it.
    let mut holds = Holds::default();
    holds.1 = [ids[0].clone()].into_iter().collect();
    tick_at(&app, &mut holds, now + 20_000).await.unwrap();
    assert!(fake.calls().iter().all(|c| !c.contains(&ids[0]) && !c.starts_with("resume")), "nothing of alice's");
    let path = format!("/api/projects/{project}/workspaces/{}", ids[0]);
    let r = call(&base, "bob", "GET", &path, None).await;
    assert_eq!(
        refused(&r),
        (403, SUSPENDED, "This cloud workspace can't start: the account that made it is suspended.")
    );

    hub.set_standing(TestHub::standing("alice"));
    assert_eq!(call(&base, "bob", "GET", &path, None).await.0, 200);
    drive(&app, now + 30_000).await;
    assert!(fake.calls().iter().any(|c| c.starts_with("resume")));
}
