//! Warming a project's agents box from chat's typing ping (M262), on the
//! base's ready box (`hosted::wake_tests`).

use std::{
    sync::atomic::Ordering::SeqCst,
    time::{Duration, Instant},
};

use arugula_control_wire::hosted::WakeAnswer;
use arugula_e2e::now_ms;

use crate::{
    compute::State,
    hosted::{
        wake_tests::{once_ready, ready_with_hub},
        worker,
    },
};

/// M262: someone who may ask `@arugula` starts writing it in the project's
/// channel, and the page's typing ping says `warm`. The paused box is
/// resumed once, by the worker, while they write; another warm ping
/// within the minute (theirs or anyone's) asks nothing, and a viewer's
/// never does. With the box gone, a ping does nothing.
#[tokio::test]
async fn an_arugula_being_written_wakes_the_agents_box_once() {
    use crate::chat::typing::{Warm, warm};
    crate::register();
    let (hub, fake, app, project, id) = ready_with_hub().await;
    hub.flag("team:t1", true);
    let room = crate::db::chat::Room {
        id: "ch_t1".into(),
        scope: "team:t1".into(),
        kind: "channel".into(),
        name: "general".into(),
        machine: None,
        target: None,
        created_by: "alice".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &["alice", "bob", "vic"], Some(&project)).unwrap();
    fake.pause_all();
    worker::start(app.clone());

    let base = format!("http://{}", {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap();
        let router = crate::testing::router(app.clone());
        tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
        a
    });
    let ping = |who: &'static str, warm: bool| {
        let base = base.clone();
        async move {
            let body = warm.then(|| serde_json::json!({ "warm": true }));
            crate::chat_tests::call(&base, who, "POST", "/api/chat/rooms/ch_t1/typing", body).await.0
        }
    };
    // A viewer's warm ping, and a plain one: nothing.
    assert_eq!(ping("vic", true).await, 204);
    assert_eq!(ping("alice", false).await, 204);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.resumes.load(SeqCst), 0);

    // Alice's warm ping: the box is resumed in the background.
    assert_eq!(ping("alice", true).await, 204);
    let until = Instant::now() + Duration::from_secs(10);
    while fake.resumes.load(SeqCst) == 0 {
        assert!(Instant::now() < until, "never woken");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(fake.boxes()[0].state, State::Running);
    once_ready(&app, &project, &id).await;

    // Within the minute: nothing asked, from anyone.
    let now = now_ms();
    assert_eq!(warm(&app, &room, "bob", now).await.unwrap(), (Warm::Soon, None));
    assert_eq!(ping("alice", true).await, 204);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.resumes.load(SeqCst), 1, "one resume");

    // A minute on, it may again; with no box, nothing does.
    let (w, a) = warm(&app, &room, "bob", now + 61_000).await.unwrap();
    assert_eq!((w, a), (Warm::Wake { project: project.clone(), workspace: id.clone() }, Some(WakeAnswer::Awake)));
    crate::db::sql::Sql::execute(&app.db.c(), "UPDATE hosted_workspaces SET deleted_at = 1", crate::db::sql::params![])
        .unwrap();
    let resumes = fake.resumes.load(SeqCst);
    assert_eq!(warm(&app, &room, "bob", now + 200_000).await.unwrap(), (Warm::NoBox, None));
    assert_eq!(fake.resumes.load(SeqCst), resumes);
}
