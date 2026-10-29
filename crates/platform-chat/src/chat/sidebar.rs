//! A person's chat sidebar (#120): the rooms they starred, sections of
//! their own with rooms in them, what they collapsed and how each group is
//! sorted. One document per account ([`ChatSidebar`], `chat_sidebars`),
//! so every page and the phone draw the same, and a change tells the
//! account's other pages ([`ChatLive::Sidebar`](wire::ChatLive)).
//!
//! The platform keeps the document and its shape, not the meaning of a room
//! id in it: a room the reader no longer sees stays listed here and the
//! page skips it, so a room that comes back (a rejoined team) is where they
//! left it.

use std::{collections::HashSet, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::chat::ChatLive;
use arugula_control_wire::chat_sidebar::{ChatSidebar, MAX_PLACED, MAX_SECTION_NAME, MAX_SECTIONS};
use arugula_e2e::now_ms;
use axum::{Json, extract::State, http::StatusCode};

use super::{J, gate};
use crate::{Platform, auth::Person};

/// Collapsed groups and sorts kept, and the longest key or room id.
const MAX_KEYS: usize = 200;
const MAX_ID: usize = 80;

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `s` as it's kept: a room in one place only (starred first, then the
/// earliest section), repeated and malformed ids dropped, and the limits
/// held. What a page never sends (a section with no name, too many) is
/// refused.
pub(crate) fn tidy(mut s: ChatSidebar) -> Result<ChatSidebar, ApiError> {
    if s.sections.len() > MAX_SECTIONS {
        return Err(err(StatusCode::BAD_REQUEST, &format!("at most {MAX_SECTIONS} sections")));
    }
    let mut placed = HashSet::new();
    s.starred.retain(|r| id_ok(r) && placed.insert(r.clone()));
    let mut ids = HashSet::new();
    for sec in &mut s.sections {
        sec.name = sec.name.trim().to_owned();
        if sec.name.is_empty() || sec.name.chars().count() > MAX_SECTION_NAME {
            return Err(err(
                StatusCode::BAD_REQUEST,
                &format!("a section's name is 1 to {MAX_SECTION_NAME} characters"),
            ));
        }
        if !id_ok(&sec.id) || !ids.insert(sec.id.clone()) {
            return Err(err(StatusCode::BAD_REQUEST, "a section needs an id of its own"));
        }
        sec.rooms.retain(|r| id_ok(r) && placed.insert(r.clone()));
    }
    if placed.len() > MAX_PLACED {
        return Err(err(StatusCode::BAD_REQUEST, &format!("at most {MAX_PLACED} rooms in the sidebar's sections")));
    }
    let mut seen = HashSet::new();
    s.collapsed.retain(|k| id_ok(k) && seen.insert(k.clone()));
    s.collapsed.truncate(MAX_KEYS);
    s.sorts.retain(|k, _| id_ok(k));
    while s.sorts.len() > MAX_KEYS {
        s.sorts.pop_last();
    }
    Ok(s)
}

/// `GET /api/chat/sidebar`: the account's, empty until it saves one.
pub async fn get(State(app): State<Arc<Platform>>, s: Person) -> J<ChatSidebar> {
    gate(&app, &s.account).await?;
    let doc = match app.db.chat_sidebar(&s.account)? {
        // A document this build can't read is as if there were none.
        Some(d) => serde_json::from_str(&d).unwrap_or_default(),
        None => ChatSidebar::default(),
    };
    Ok(Json(doc))
}

/// `PUT /api/chat/sidebar`: replace it; answered with what was kept.
pub async fn put(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<ChatSidebar>) -> J<ChatSidebar> {
    gate(&app, &s.account).await?;
    let kept = tidy(b)?;
    app.db.set_chat_sidebar(&s.account, &serde_json::to_string(&kept).map_err(anyhow::Error::from)?, now_ms())?;
    app.chat.publish(&app.db, ChatLive::Sidebar {}, Some(s.account));
    Ok(Json(kept))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_control_wire::chat_sidebar::{ChatSection, ChatSort};

    fn sec(id: &str, name: &str, rooms: &[&str]) -> ChatSection {
        ChatSection { id: id.into(), name: name.into(), rooms: rooms.iter().map(|r| (*r).into()).collect() }
    }

    #[test]
    fn a_room_is_in_one_place() {
        let kept = tidy(ChatSidebar {
            starred: vec!["a".into(), "a".into(), "b".into(), "".into()],
            sections: vec![sec("s1", " Ops ", &["b", "c", "c"]), sec("s2", "Projects", &["c", "d"])],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(kept.starred, ["a", "b"]);
        assert_eq!(kept.sections[0].name, "Ops");
        assert_eq!(kept.sections[0].rooms, ["c"]);
        assert_eq!(kept.sections[1].rooms, ["d"]);
    }

    #[test]
    fn what_a_page_never_sends_is_refused() {
        let bad = |sections: Vec<ChatSection>| tidy(ChatSidebar { sections, ..Default::default() }).is_err();
        assert!(bad(vec![sec("s1", "  ", &[])]));
        assert!(bad(vec![sec("s1", &"x".repeat(MAX_SECTION_NAME + 1), &[])]));
        assert!(bad(vec![sec("s1", "A", &[]), sec("s1", "B", &[])]));
        assert!(bad((0..=MAX_SECTIONS).map(|i| sec(&format!("s{i}"), "A", &[])).collect()));
    }

    #[test]
    fn collapsed_and_sorts_are_bounded() {
        let mut s = ChatSidebar { collapsed: vec!["starred".into(), "starred".into()], ..Default::default() };
        for i in 0..MAX_KEYS + 5 {
            s.sorts.insert(format!("k{i:04}"), ChatSort::Recent);
        }
        let kept = tidy(s).unwrap();
        assert_eq!(kept.collapsed, ["starred"]);
        assert_eq!(kept.sorts.len(), MAX_KEYS);
    }

    #[tokio::test]
    async fn it_follows_the_account_and_tells_only_its_pages() {
        use crate::chat_tests::{TestHub, call, seed, serve};
        use serde_json::json;
        let hub = std::sync::Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        let (st, v) = call(&base, "a", "GET", "/api/chat/sidebar", None).await;
        assert_eq!((st, v), (200, json!({ "starred": [], "sections": [], "collapsed": [], "sorts": {} })));
        let body = json!({
            "starred": ["ch_general"],
            "sections": [{ "id": "s_ops", "name": "Ops", "rooms": ["th_0_0", "ch_general"] }],
            "collapsed": ["s_ops"],
            "sorts": { "starred": "recent" },
        });
        let (st, kept) = call(&base, "a", "PUT", "/api/chat/sidebar", Some(body)).await;
        assert_eq!(st, 200, "{kept}");
        // Starred wins over the section it was also dragged into.
        assert_eq!(kept["sections"][0]["rooms"], json!(["th_0_0"]));
        // Another device of the same account reads what was kept; another
        // account's is its own.
        assert_eq!(call(&base, "a", "GET", "/api/chat/sidebar", None).await.1, kept);
        assert_eq!(call(&base, "b", "GET", "/api/chat/sidebar", None).await.1["starred"], json!([]));
        // The event is the account's alone.
        let events = app.db.chat_events_after(0, 10).unwrap();
        let told: Vec<_> = events.iter().filter(|e| e.body.contains("sidebar")).map(|e| e.to.clone()).collect();
        assert_eq!(told, [Some("a".to_owned())]);
        // Someone chat is off for has none to keep.
        let (st, _) = call(&base, "z", "PUT", "/api/chat/sidebar", Some(json!({}))).await;
        assert_eq!(st, 403);
        let (st, _) =
            call(&base, "a", "PUT", "/api/chat/sidebar", Some(json!({ "sections": [{ "id": "x", "name": "" }] })))
                .await;
        assert_eq!(st, 400);
    }
}
