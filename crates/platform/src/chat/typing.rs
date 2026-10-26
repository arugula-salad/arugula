//! The typing indicator (#111): a page pings while its composer's text
//! changes, and the room's other open readers hear it. A ping is a
//! short-lived `chat_events` row (kind `typing`), so it crosses machines as
//! messages do (M144): a reader whose stream is on the platform's other
//! machine hears it. It's sent only while fresh ([`wire::TYPING_MS`]), never
//! replayed to a stream that resumes (`Hub::missed` reads `live` rows only),
//! and pruned within seconds (`Db::prune_typing`). A page shows a name for
//! [`wire::TYPING_MS`] after its last ping.
//!
//! **Warming the box** (M262, thread = session, Arugula #129): a ping whose
//! composer asks `@arugula` says `warm` ([`wire::ChatTyping`]), and the
//! machine it reaches wakes the project's agents box ([`warm`]) while the
//! message is still being written, so a paused box (about a second to
//! resume) is up by the time it's sent. Only where `@arugula` works (a
//! project's channel, or the thread on its issue or PR; never a DM), only
//! for someone it would start a session for ([`crate::triggers::may_start`]),
//! only the box its sessions go to (`Db::agents_run_on`: the one chosen,
//! else the only one), and at most once a minute per project across the
//! platform's machines (`Db::take_warm`). The wake is the hub's own
//! ([`crate::hosted::wake::wake`]): one resume, done by the worker, and a
//! woken box is `waking` until its daemon dials in.

use super::*;
use crate::{db::sessions::AgentsOn, triggers::Cannot};

/// At most one ping per account per room in this long: the page sends one
/// every [`wire::TYPING_PING_MS`], so this only holds back a client that
/// doesn't.
const GAP_MS: u64 = 1_000;

impl Hub {
    /// Tell every machine's streams that `account` is typing in `room`.
    /// Whether it was sent (a ping inside the gap isn't).
    fn typing(&self, db: &Db, room: &str, account: &str, name: &str, now: u64) -> bool {
        {
            let mut t = self.typed.lock().unwrap();
            if t.len() > 50_000 {
                t.retain(|_, at| now.saturating_sub(*at) < GAP_MS);
            }
            let key = (account.to_owned(), room.to_owned());
            if t.get(&key).is_some_and(|at| now.saturating_sub(*at) < GAP_MS) {
                return false;
            }
            t.insert(key, now);
        }
        let ev = ChatLive::Typing { room: room.to_owned(), account: account.to_owned(), name: name.to_owned() };
        let body = data(&ev);
        let e = NewChatEvent { kind: "typing", body: &body, to: None, at: now };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a typing ping");
        }
        self.catch_up(db);
        true
    }

    /// Whether the stream `id` has `room` open (it hears typing there only).
    pub(super) fn stream_has_open(&self, id: &str, room: &str) -> bool {
        self.streams.lock().unwrap().get(id).is_some_and(|s| s.open.iter().any(|r| r == room))
    }
}

/// `POST /api/chat/rooms/{id}/typing`: I'm typing here. For a room I see
/// and may write in; a read-only or watched room has nobody to tell. With
/// `warm` ([`wire::ChatTyping`]), the project's agents box is woken too,
/// in the background ([`warm`]); with `show` false (a reply thread's
/// composer), only that: nothing is kept or sent as typing.
pub async fn typing(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    body: Option<Json<wire::ChatTyping>>,
) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    if watching(&app, &facts, &s.account, &room)? || !writable(&app, &facts, &room, &s.account)? {
        return Err(err(StatusCode::CONFLICT, "you can't write here"));
    }
    let ping = body.map(|Json(b)| b).unwrap_or_default();
    let now = now_ms();
    if ping.show {
        let name = name_of(&app, &s.account).await?;
        app.chat.typing(&app.db, &room.id, &s.account, &name, now);
    }
    if ping.warm {
        let account = s.account.clone();
        tokio::spawn(async move {
            if let Err(e) = warm(&app, &room, &account, now).await {
                tracing::warn!(room = room.id, error = %format!("{e:#}"), "chat: warming the agents box");
            }
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

/// What a warm ping comes to ([`warm_for`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Warm {
    /// Not a room `@arugula` works in: a DM, a pane's room, a room outside
    /// a project.
    NotHere,
    /// The sender couldn't start a session there.
    Cannot(Cannot),
    /// The project has no box its sessions would go to: none, or several
    /// and none chosen.
    NoBox,
    /// This project's box was warmed within the last minute.
    Soon,
    /// Wake this box of this project (the minute is taken).
    Wake { project: String, workspace: String },
}

/// Whether `account`'s warm ping in `room` at `now` wakes a box, and which.
/// The minute is only taken for a ping that would wake one, so a viewer's
/// ping never holds back an editor's.
pub(crate) async fn warm_for(app: &Platform, room: &Room, account: &str, now: u64) -> anyhow::Result<Warm> {
    if !super::sessions::works_in(room) {
        return Ok(Warm::NotHere);
    }
    let Some(project) = app.db.chat_room_project(&room.id)? else { return Ok(Warm::NotHere) };
    if let Some(why) = crate::triggers::may_start(app, &project, account).await? {
        return Ok(Warm::Cannot(why));
    }
    let workspace = match app.db.agents_run_on(&project)? {
        AgentsOn::Chosen(w) | AgentsOn::Only(w) => w,
        AgentsOn::Ask(_) | AgentsOn::Nowhere => return Ok(Warm::NoBox),
    };
    if !app.db.take_warm(&project, now)? {
        return Ok(Warm::Soon);
    }
    Ok(Warm::Wake { project, workspace })
}

/// A warm ping (module docs): wake the project's agents box when
/// [`warm_for`] says to, and what the wake answered.
pub(crate) async fn warm(
    app: &Platform,
    room: &Room,
    account: &str,
    now: u64,
) -> anyhow::Result<(Warm, Option<arugula_control_wire::hosted::WakeAnswer>)> {
    let w = warm_for(app, room, account, now).await?;
    let Warm::Wake { project, workspace } = &w else { return Ok((w, None)) };
    let a = crate::hosted::wake::wake(app, project, workspace).await?;
    tracing::info!(room = room.id, %workspace, answer = ?a, "chat: an @arugula being written warmed the agents box");
    Ok((w, Some(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_goes_to_other_streams_and_not_twice_in_a_second() {
        let db = Db::test();
        let hub = Hub::default();
        let mut rx = hub.tx.subscribe();
        let now = now_ms();
        assert!(hub.typing(&db, "ch_1", "a1", "Alex", now));
        assert!(!hub.typing(&db, "ch_1", "a1", "Alex", now + 500), "inside the gap");
        assert!(hub.typing(&db, "ch_2", "a1", "Alex", now + 500), "another room");
        assert!(hub.typing(&db, "ch_1", "a1", "Alex", now + 1_000));
        let first = rx.try_recv().unwrap();
        assert_eq!(first.n, 0, "not numbered: nothing replays it");
        assert!(matches!(&first.ev, ChatLive::Typing { room, account, .. } if room == "ch_1" && account == "a1"));
    }

    #[test]
    fn a_machine_that_was_away_hears_only_fresh_pings_and_old_ones_are_pruned() {
        let db = Db::test();
        let a = Hub::default();
        let b = Hub::default();
        let mut rx = b.tx.subscribe();
        let now = now_ms();
        let ping = |room: &str, at: u64| {
            let ev = ChatLive::Typing { room: room.into(), account: "a1".into(), name: "Alex".into() };
            db.add_chat_event(&NewChatEvent { kind: "typing", body: &data(&ev), to: None, at }).unwrap();
        };
        ping("ch_old", now - crate::db::chat::TYPING_KEPT_MS - 1_000);
        // Another machine's ping reaches this one through the table.
        assert!(a.typing(&db, "ch_new", "a1", "Alex", now));
        b.catch_up(&db);
        let heard = rx.try_recv().unwrap();
        assert!(matches!(&heard.ev, ChatLive::Typing { room, .. } if room == "ch_new"));
        assert!(rx.try_recv().is_err(), "a stale ping isn't sent");
        // A resume reads `live` rows only.
        let (oldest, _) = db.chat_event_bounds().unwrap();
        assert!(a.missed(&db, &format!("c-{}", oldest - 1)).is_none_or(|(_, outs)| outs.is_empty()));
        // The stale one goes; the fresh one stays for a while.
        assert_eq!(db.prune_typing(now + 1).unwrap(), 1);
        assert_eq!(db.chat_events_after(0, 10).unwrap().len(), 1);
        assert_eq!(db.prune_typing(now + crate::db::chat::TYPING_KEPT_MS + 1).unwrap(), 1);
    }

    #[test]
    fn only_a_stream_with_the_room_open_hears_it() {
        let hub = Hub::default();
        hub.streams
            .lock()
            .unwrap()
            .insert("s1".into(), StreamInfo { account: "b".into(), open: vec!["ch_1".into()], thread: None });
        hub.streams
            .lock()
            .unwrap()
            .insert("s2".into(), StreamInfo { account: "c".into(), open: Vec::new(), thread: None });
        assert!(hub.stream_has_open("s1", "ch_1"));
        assert!(!hub.stream_has_open("s1", "ch_2"));
        assert!(!hub.stream_has_open("s2", "ch_1"));
    }

    /// M262: a warm ping wakes the project's box only for someone who could
    /// ask `@arugula` there (not a viewer, not a member off the beta's
    /// list, not someone outside), and once a minute per project, whoever
    /// sends it. A refused ping doesn't take the minute.
    #[tokio::test]
    async fn a_warm_ping_wakes_for_someone_who_could_ask_once_a_minute() {
        let (_hub, app, _base, t, _u, _keys) =
            crate::sessions_hydrate_tests::platform(&["https://github.com/o/app"]).await;
        let room = app.db.chat_room("ch_general").unwrap().unwrap();
        let now = now_ms();
        assert_eq!(warm_for(&app, &room, "x", now).await.unwrap(), Warm::Cannot(Cannot::Viewer));
        assert_eq!(warm_for(&app, &room, "y", now).await.unwrap(), Warm::Cannot(Cannot::Unlisted));
        assert_eq!(warm_for(&app, &room, "c", now).await.unwrap(), Warm::Cannot(Cannot::Outside));
        let wake = Warm::Wake { project: t.clone(), workspace: "hw_dev".into() };
        assert_eq!(warm_for(&app, &room, "b", now).await.unwrap(), wake, "the refused ones took nothing");
        assert_eq!(warm_for(&app, &room, "b", now + 3_000).await.unwrap(), Warm::Soon);
        assert_eq!(warm_for(&app, &room, "a", now + 59_000).await.unwrap(), Warm::Soon, "per project");
        assert_eq!(warm_for(&app, &room, "a", now + 60_001).await.unwrap(), wake);

        // The minute is the database's, so it holds whichever machine asks;
        // it's pruned once it's past.
        assert!(!app.db.take_warm(&t, now + 61_000).unwrap());
        assert!(app.db.take_warm("prj_other", now + 61_000).unwrap(), "another project's own");
        app.db.prune_typing(now + 200_000).unwrap();
        assert!(app.db.chat_events_after(0, 10).unwrap().iter().all(|e| e.kind != "warm"));
    }

    /// M262: a reply thread's composer sends a warm-only ping (`show`
    /// false). It isn't typing: no row is kept as one and no stream hears
    /// it. It still goes through the same checks and the same minute: a
    /// viewer's takes nothing, an editor's takes the project's minute once.
    #[tokio::test]
    async fn a_warm_only_ping_isnt_typing() {
        let (_hub, app, base, t, _u, _keys) =
            crate::sessions_hydrate_tests::platform(&["https://github.com/o/app"]).await;
        let mut rx = app.chat.tx.subscribe();
        let ping = |who: &'static str, body: serde_json::Value| {
            let base = base.clone();
            async move {
                crate::chat_tests::call(&base, who, "POST", "/api/chat/rooms/ch_general/typing", Some(body)).await.0
            }
        };
        let kinds = || app.db.chat_events_after(0, 100).unwrap().into_iter().map(|e| e.kind).collect::<Vec<_>>();
        let quiet = serde_json::json!({ "warm": true, "show": false });
        assert_eq!(ping("x", quiet.clone()).await, 204);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(kinds().is_empty(), "a viewer's: nothing, not even the minute");
        assert_eq!(ping("b", quiet.clone()).await, 204);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while kinds().is_empty() {
            assert!(std::time::Instant::now() < until, "the minute was never taken");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(kinds(), ["warm"], "not typing");
        assert!(rx.try_recv().is_err(), "nobody hears it");
        assert!(!app.db.take_warm(&t, now_ms()).unwrap(), "the same minute as a shown ping's");
        assert_eq!(ping("a", quiet).await, 204);
        // A plain ping, or one that only shows, is typing as before.
        assert_eq!(ping("a", serde_json::json!({})).await, 204);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(kinds(), ["warm", "typing"], "the second warm-only ping took nothing");
        assert!(matches!(&rx.try_recv().unwrap().ev, ChatLive::Typing { account, .. } if account == "a"));
    }

    /// M262: machines asking at once (each its own connection) take a
    /// project's minute once between them.
    #[test]
    fn the_minute_is_taken_once_by_machines_at_once() {
        let db = Db::test();
        let now = now_ms();
        let took = std::thread::scope(|s| {
            let all: Vec<_> = (0..8).map(|_| s.spawn(|| db.take_warm("prj_a", now).unwrap())).collect();
            all.into_iter().map(|h| h.join().unwrap()).filter(|t| *t).count()
        });
        assert_eq!(took, 1);
    }

    /// M262: never in a DM; nothing when the project has no box its
    /// sessions would go to (none, or several and none chosen); the chosen
    /// one when there is one.
    #[tokio::test]
    async fn a_warm_ping_in_a_dm_or_with_no_box_wakes_nothing() {
        use crate::db::sql::{Sql as _, params};
        let (_hub, app, _base, t, _u, _keys) =
            crate::sessions_hydrate_tests::platform(&["https://github.com/o/app"]).await;
        let dm = Room {
            id: "dm_ab".into(),
            scope: "team:t".into(),
            kind: "dm".into(),
            name: "dm_ab".into(),
            machine: None,
            target: None,
            created_by: "a".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        app.db.add_chat_room(&dm, &["a", "b"], None).unwrap();
        let now = now_ms();
        assert_eq!(warm_for(&app, &dm, "b", now).await.unwrap(), Warm::NotHere);

        let room = app.db.chat_room("ch_general").unwrap().unwrap();
        crate::sessions_hydrate_tests::add_box(&app, &t, "hw_stage", "staging", &[], None);
        assert_eq!(warm_for(&app, &room, "b", now).await.unwrap(), Warm::NoBox, "two, none chosen");
        assert!(app.db.set_agents_run_on(&t, Some("hw_stage")).unwrap());
        let wake = Warm::Wake { project: t.clone(), workspace: "hw_stage".into() };
        assert_eq!(warm_for(&app, &room, "b", now).await.unwrap(), wake, "the chosen one");

        app.db.c().execute("UPDATE hosted_workspaces SET deleted_at = 1", params![]).unwrap();
        assert_eq!(warm_for(&app, &room, "b", now + 120_000).await.unwrap(), Warm::NoBox);
        assert!(app.db.take_warm(&t, now + 120_000).unwrap(), "no box took no minute");
    }

    #[test]
    fn a_page_with_two_chat_blocks_has_both_rooms_open() {
        let hub = Hub::default();
        hub.streams.lock().unwrap().insert(
            "s1".into(),
            StreamInfo { account: "b".into(), open: vec!["ch_1".into(), "ch_2".into()], thread: None },
        );
        assert!(hub.stream_has_open("s1", "ch_1") && hub.stream_has_open("s1", "ch_2"));
        assert!(hub.has_open("b", "ch_2"), "no push for either");
        assert!(!hub.has_open("c", "ch_2"));
    }
}
