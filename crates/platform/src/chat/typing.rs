//! The typing indicator (#111): a page pings while its composer's text
//! changes, and the room's other open readers hear it. Nothing is kept:
//! the ping isn't a `chat_events` row, so it isn't replayed to a stream
//! that resumes, and on a platform running on several machines only the
//! readers whose stream is on the machine that got the ping hear it. A
//! page shows a name for [`wire::TYPING_MS`] after its last ping.

use super::*;

/// At most one ping per account per room in this long: the page sends one
/// every [`wire::TYPING_PING_MS`], so this only holds back a client that
/// doesn't.
const GAP_MS: u64 = 1_000;

impl Hub {
    /// Tell this machine's streams that `account` is typing in `room`.
    /// Whether it was sent (a ping inside the gap isn't).
    fn typing(&self, room: &str, account: &str, name: &str, now: u64) -> bool {
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
        // `n: 0`: not an event of the log, so no stream numbers it.
        let _ = self.tx.send(Arc::new(Out { n: 0, ev, to: None }));
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
    app.chat.typing(&room.id, &s.account, &name, now_ms());
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_goes_to_other_streams_and_not_twice_in_a_second() {
        let hub = Hub::default();
        let mut rx = hub.tx.subscribe();
        assert!(hub.typing("ch_1", "a1", "Alex", 10_000));
        assert!(!hub.typing("ch_1", "a1", "Alex", 10_500), "inside the gap");
        assert!(hub.typing("ch_2", "a1", "Alex", 10_500), "another room");
        assert!(hub.typing("ch_1", "a1", "Alex", 11_000));
        let first = rx.try_recv().unwrap();
        assert_eq!(first.n, 0, "not numbered: nothing replays it");
        assert!(matches!(&first.ev, ChatLive::Typing { room, account, .. } if room == "ch_1" && account == "a1"));
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
