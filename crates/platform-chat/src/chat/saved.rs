//! Mark as unread and saved for later (#116). Both are per account and
//! private, and both reach the account's other pages over the live stream
//! (events `to` the account): marking unread moves the room's read mark
//! back (the same `read` event as reading, now going backwards), and a
//! save, a "done", a removal or a reminder coming due is a `saved` event
//! that makes the page read its list again.
//!
//! A reminder is one push, once: [`remind`] runs in chat's background loop
//! on every machine, and [`Db::claim_chat_reminders`] hands each due item
//! to exactly one of them.

use super::*;
use wire::{ChatSave, ChatSaved, ChatSavedDone, ChatSavedItem, ChatUnread, SAVED_REMIND_MAX_MS};

/// Due reminders sent per pass of the loop, so a backlog (a machine that
/// was down) goes out over a few passes rather than all at once.
const REMINDERS_PER_PASS: u32 = 50;

/// `POST /api/chat/rooms/{id}/unread`: the read mark goes to just before
/// `seq`, so that message and everything after it by others is unread
/// again and the page draws its "New" line there.
pub async fn unread(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatUnread>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let m = message_in(&app, &room, b.seq)?;
    if m.at < from {
        return Err(err(StatusCode::NOT_FOUND, "no such message"));
    }
    let mark = b.seq.saturating_sub(1);
    app.db.set_chat_read_to(&room.id, &s.account, mark)?;
    app.chat.publish(&app.db, ChatLive::Read { room: room.id.clone(), seq: mark }, Some(s.account.clone()));
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// The message `seq` of a room, for saving: one the reader may read.
async fn saveable(app: &Platform, account: &str, id: &str, seq: i64) -> Result<Room, ApiError> {
    gate(app, account).await?;
    let (room, from, _) = room_for(app, account, id).await?;
    let m = message_in(app, &room, seq)?;
    if m.at < from || m.deleted.is_some() {
        return Err(err(StatusCode::NOT_FOUND, "no such message"));
    }
    Ok(room)
}

fn saved_changed(app: &Platform, account: &str) {
    app.chat.publish(&app.db, ChatLive::Saved {}, Some(account.to_owned()));
}

/// `PUT /api/chat/rooms/{id}/messages/{seq}/saved`: save it, with an
/// optional reminder.
pub async fn save(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    body: Option<Json<ChatSave>>,
) -> Result<StatusCode, ApiError> {
    let room = saveable(&app, &s.account, &id, seq).await?;
    let now = now_ms();
    let remind = body.and_then(|Json(b)| b.remind_at);
    if remind.is_some_and(|at| at <= now || at > now + SAVED_REMIND_MAX_MS) {
        return Err(err(StatusCode::BAD_REQUEST, "a reminder is for a time ahead, within a year"));
    }
    app.db.save_chat_message(&s.account, &room.id, seq, now, remind)?;
    saved_changed(&app, &s.account);
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH .../saved`: done, or not.
pub async fn done(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<ChatSavedDone>,
) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    if !app.db.set_chat_saved_done(&s.account, &id, seq, b.done.then(now_ms))? {
        return Err(err(StatusCode::NOT_FOUND, "that isn't saved"));
    }
    saved_changed(&app, &s.account);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE .../saved`: not saved any more (nothing to say if it wasn't).
pub async fn unsave(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    if app.db.unsave_chat_message(&s.account, &id, seq)? {
        saved_changed(&app, &s.account);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/chat/saved`: the reader's saved messages, but those of rooms
/// they no longer see, from before their floor, or deleted.
pub async fn list(State(app): State<Arc<Platform>>, s: Person) -> J<ChatSaved> {
    gate(&app, &s.account).await?;
    let rows = app.db.chat_saved(&s.account)?;
    if rows.is_empty() {
        return Ok(Json(ChatSaved { items: vec![] }));
    }
    let (seen, _) = rooms_seen(&app, &s.account).await?;
    let floors: HashMap<&str, u64> = seen.iter().map(|(r, from)| (r.id.as_str(), *from)).collect();
    let mut kept = Vec::new();
    for r in rows {
        let Some(from) = floors.get(r.room.as_str()) else { continue };
        let Some(m) = app.db.chat_message(&r.room, r.seq)? else { continue };
        if m.at >= *from && m.deleted.is_none() {
            kept.push((r, m));
        }
    }
    let shower = Shower::of(&app, &s.account, &kept.iter().map(|(_, m)| m).collect::<Vec<_>>()).await?;
    let items = kept
        .iter()
        .map(|(r, m)| ChatSavedItem {
            room: r.room.clone(),
            seq: r.seq,
            saved_at: r.saved_at,
            done: r.done,
            remind_at: r.remind_at,
            reminded: r.reminded,
            message: shower.show(m),
        })
        .collect();
    Ok(Json(ChatSaved { items }))
}

/// Push the reminders that came due, once each, to the account that set
/// them (every subscription it has, as chat's pushes go), and tell its
/// pages so the item shows as reminded. A room the account no longer sees
/// has nothing to say.
pub async fn remind(app: &Platform) {
    let due = match app.db.claim_chat_reminders(now_ms(), REMINDERS_PER_PASS) {
        Ok(d) => d,
        Err(e) => return tracing::warn!(error = %e, "chat: finding due reminders"),
    };
    for (account, id, seq) in due {
        if let Err(e) = remind_one(app, &account, &id, seq).await {
            tracing::warn!(error = %e, %account, "chat: a saved-message reminder didn't go");
        }
    }
}

async fn remind_one(app: &Platform, account: &str, id: &str, seq: i64) -> anyhow::Result<()> {
    saved_changed(app, account);
    let Ok((room, from, facts)) = room_for(app, account, id).await else { return Ok(()) };
    let Some(m) = app.db.chat_message(&room.id, seq)?.filter(|m| m.at >= from && m.deleted.is_none()) else {
        return Ok(());
    };
    let shown = Shower::of(app, account, &[&m]).await?.show(&m);
    let place = listed(app, &facts, account, &room, from)?;
    let place = match place.kind {
        ChatKind::Channel => format!("#{}", room.name),
        _ => room.name.clone(),
    };
    let msg = json!({
        "chat": { "room": room.id, "seq": seq },
        "title": format!("Reminder: {} in {place}", shown.name),
        "body": shown.text.chars().take(wire::PUSH_CHARS).collect::<String>(),
        "tag": format!("saved-{}-{seq}", room.id), "control": true,
    });
    app.hub.push_with(account, &msg, PUSHES).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_tests::{TestHub, call, hears, open_live, seed, serve};
    use serde_json::json;

    async fn said(base: &str, who: &str, text: &str) {
        let (st, v) =
            call(base, who, "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": text }))).await;
        assert_eq!(st, 200, "{v}");
    }

    fn room_of(v: &serde_json::Value) -> (i64, u64) {
        (v["read"].as_i64().unwrap(), v["unread"].as_u64().unwrap())
    }

    #[tokio::test]
    async fn marking_unread_moves_the_mark_back_for_that_account_and_its_pages() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        for t in ["one", "two", "three"] {
            said(&base, "a", t).await;
        }
        let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/read", Some(json!({ "seq": 3 }))).await;
        assert_eq!(room_of(&v), (3, 0));
        let b_other_page = open_live(&base, "b", None).await;
        let a_page = open_live(&base, "a", None).await;
        hears(&b_other_page, "\"type\":\"hello\"", Duration::from_secs(5)).await;
        hears(&a_page, "\"type\":\"hello\"", Duration::from_secs(5)).await;

        let (st, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/unread", Some(json!({ "seq": 2 }))).await;
        assert_eq!((st, room_of(&v)), (200, (1, 2)), "{v}");
        let (_, rooms) = call(&base, "b", "GET", "/api/chat", None).await;
        let general = rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap().clone();
        assert_eq!(room_of(&general), (1, 2), "the room list agrees");
        hears(&b_other_page, "\"type\":\"read\",\"room\":\"ch_general\",\"seq\":1", Duration::from_secs(4)).await;
        // Reading is forward-only still; the author's own mark is theirs.
        let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/read", Some(json!({ "seq": 1 }))).await;
        assert_eq!(room_of(&v), (1, 2));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!a_page.lock().unwrap().contains("\"type\":\"read\""), "private to b: {}", a_page.lock().unwrap());
        let (st, _) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/unread", Some(json!({ "seq": 99 }))).await;
        assert_eq!(st, 404, "no such message");
        let (st, _) = call(&base, "b", "POST", "/api/chat/rooms/th_0_0/unread", Some(json!({ "seq": 1 }))).await;
        assert_eq!(st, 404, "a room b doesn't see");
    }

    #[tokio::test]
    async fn saved_messages_are_private_listed_and_can_be_done_or_removed() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        said(&base, "a", "first").await;
        said(&base, "a", "second").await;
        let b_page = open_live(&base, "b", None).await;
        hears(&b_page, "\"type\":\"hello\"", Duration::from_secs(5)).await;

        let save = |who: &'static str, seq: i64| {
            let base = base.clone();
            async move { call(&base, who, "PUT", &format!("/api/chat/rooms/ch_general/messages/{seq}/saved"), None).await.0 }
        };
        assert_eq!(save("b", 1).await, 204);
        assert_eq!(save("b", 2).await, 204);
        assert_eq!(save("b", 9).await, 404, "no such message");
        hears(&b_page, "\"type\":\"saved\"", Duration::from_secs(4)).await;
        let (_, v) = call(&base, "b", "GET", "/api/chat/saved", None).await;
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.iter().map(|i| i["seq"].as_i64().unwrap()).collect::<Vec<_>>(), [2, 1], "newest first");
        assert_eq!(items[0]["message"]["text"], "second");
        let (_, v) = call(&base, "a", "GET", "/api/chat/saved", None).await;
        assert_eq!(v["items"].as_array().unwrap().len(), 0, "a saved nothing");
        // A room b can't see can't be saved from.
        let (st, _) = call(&base, "b", "PUT", "/api/chat/rooms/th_0_0/messages/1/saved", None).await;
        assert_eq!(st, 404);

        let (st, _) =
            call(&base, "b", "PATCH", "/api/chat/rooms/ch_general/messages/2/saved", Some(json!({ "done": true })))
                .await;
        assert_eq!(st, 204);
        let (_, v) = call(&base, "b", "GET", "/api/chat/saved", None).await;
        let items = v["items"].as_array().unwrap();
        assert_eq!((items[0]["seq"].as_i64(), items[1]["seq"].as_i64()), (Some(1), Some(2)), "done ones last");
        assert!(items[0]["done"].is_null() && items[1]["done"].is_u64());
        let (st, _) =
            call(&base, "a", "PATCH", "/api/chat/rooms/ch_general/messages/2/saved", Some(json!({ "done": true })))
                .await;
        assert_eq!(st, 404, "not a's to finish");

        let (st, _) = call(&base, "b", "DELETE", "/api/chat/rooms/ch_general/messages/1/saved", None).await;
        assert_eq!(st, 204);
        let (_, v) = call(&base, "b", "GET", "/api/chat/saved", None).await;
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        // Deleting the message takes it from the list.
        let (st, _) = call(&base, "a", "DELETE", "/api/chat/rooms/ch_general/messages/2", None).await;
        assert_eq!(st, 200);
        let (_, v) = call(&base, "b", "GET", "/api/chat/saved", None).await;
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_reminder_pushes_once_when_due_and_not_when_done_or_in_the_future() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        for t in ["one", "two", "three"] {
            said(&base, "a", t).await;
        }
        let url = |seq: i64| format!("/api/chat/rooms/ch_general/messages/{seq}/saved");
        let soon = now_ms() + 3_600_000;
        let (st, _) = call(&base, "b", "PUT", &url(1), Some(json!({ "remind_at": soon }))).await;
        assert_eq!(st, 204);
        let (st, _) = call(&base, "b", "PUT", &url(2), Some(json!({ "remind_at": now_ms() - 5 }))).await;
        assert_eq!(st, 400, "a reminder is for later");
        let (st, _) = call(&base, "b", "PUT", &url(2), Some(json!({ "remind_at": now_ms() + 400 * 86_400_000 }))).await;
        assert_eq!(st, 400, "and within a year");
        remind(&app).await;
        assert!(hub.pushes().is_empty(), "not due");

        // Two come due (as time passing would): one is marked done first.
        let long_ago = now_ms() - 1;
        app.db.save_chat_message("b", "ch_general", 2, 1, Some(long_ago)).unwrap();
        app.db.save_chat_message("b", "ch_general", 3, 1, Some(long_ago)).unwrap();
        app.db.set_chat_saved_done("b", "ch_general", 3, Some(now_ms())).unwrap();
        remind(&app).await;
        remind(&app).await;
        let pushes = hub.pushes();
        assert_eq!(pushes.len(), 1, "once, and not the done one: {pushes:?}");
        let (to, msg, _) = &pushes[0];
        assert_eq!((to.as_str(), &msg["chat"]["seq"]), ("b", &json!(2)));
        assert_eq!(msg["title"], "Reminder: a in #ch_general");
        assert_eq!(msg["body"], "two");
        let (_, v) = call(&base, "b", "GET", "/api/chat/saved", None).await;
        let two = v["items"].as_array().unwrap().iter().find(|i| i["seq"] == 2).unwrap().clone();
        assert_eq!(two["reminded"], true);
        // Saving it again arms a new reminder.
        app.db.save_chat_message("b", "ch_general", 2, 2, Some(now_ms() - 1)).unwrap();
        remind(&app).await;
        assert_eq!(hub.pushes().len(), 2);
    }

    #[test]
    fn a_claimed_reminder_is_claimed_once_and_an_account_keeps_a_bounded_list() {
        let db = Db::test();
        db.save_chat_message("b", "r", 1, 10, Some(5)).unwrap();
        assert_eq!(db.claim_chat_reminders(6, 10).unwrap(), [("b".into(), "r".into(), 1)]);
        assert!(db.claim_chat_reminders(6, 10).unwrap().is_empty(), "the other machine finds none");
        for seq in 2..(wire::SAVED_MAX as i64 + 10) {
            db.save_chat_message("b", "r", seq, 100 + seq as u64, None).unwrap();
        }
        let kept = db.chat_saved("b").unwrap();
        assert_eq!(kept.len(), wire::SAVED_MAX);
        assert!(kept.iter().all(|r| r.seq >= 10), "the oldest went");
    }
}
