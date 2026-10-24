//! The typing indicator (#111): a page pings while its composer's text
//! changes, and the room's other open readers hear it. A ping is a
//! short-lived `chat_events` row (kind `typing`), so it crosses machines as
//! messages do (M144): a reader whose stream is on the platform's other
//! machine hears it. It's sent only while fresh ([`wire::TYPING_MS`]), never
//! replayed to a stream that resumes (`Hub::missed` reads `live` rows only),
//! and pruned within seconds (`Db::prune_typing`). A page shows a name for
//! [`wire::TYPING_MS`] after its last ping.

use super::*;

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
        self.streams.lock().unwrap().get(id).is_some_and(|s| s.open.as_deref() == Some(room))
    }
}

/// `POST /api/chat/rooms/{id}/typing`: I'm typing here. For a room I see
/// and may write in; a read-only or watched room has nobody to tell.
pub async fn typing(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    if watching(&app, &facts, &s.account, &room)? || !writable(&app, &facts, &room, &s.account)? {
        return Err(err(StatusCode::CONFLICT, "you can't write here"));
    }
    let name = name_of(&app, &s.account).await?;
    app.chat.typing(&app.db, &room.id, &s.account, &name, now_ms());
    Ok(StatusCode::NO_CONTENT)
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
        hub.streams.lock().unwrap().insert("s1".into(), StreamInfo { account: "b".into(), open: Some("ch_1".into()) });
        hub.streams.lock().unwrap().insert("s2".into(), StreamInfo { account: "c".into(), open: None });
        assert!(hub.stream_has_open("s1", "ch_1"));
        assert!(!hub.stream_has_open("s1", "ch_2"));
        assert!(!hub.stream_has_open("s2", "ch_1"));
    }
}
