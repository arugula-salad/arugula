//! Who's around, live, on arugula.stream: a People block joins its team's
//! room there (`team:<id>`) with a room token control signs here, as the
//! arugula.stream app control is configured with ([`Presence`]). The relay
//! carries the room; control keeps nothing of it.
//!
//! Only a team's members get a token for its room (a locked team's owners),
//! named as the roster names them, behind chat's gate as presence is.

use std::sync::Arc;

use arugula_control_common::err;
use axum::{Json, extract::State, http::StatusCode};
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::{J, gate};
use crate::{Platform, Presence, auth::Person};

/// How long a token lets its person in: the page asks for a fresh one on
/// every reconnect.
const TTL_S: u64 = 3600;

#[derive(Deserialize)]
pub struct Asked {
    /// `team:<id>`.
    room: String,
}

#[derive(Serialize)]
pub struct Answer {
    token: String,
    /// The relay's socket, which the page loads arugula.stream's elements from.
    relay: String,
}

/// `POST /api/chat/presence/token`: a room token for a team's room on
/// arugula.stream, to one of its members.
pub async fn token(State(app): State<Arc<Platform>>, s: Person, Json(a): Json<Asked>) -> J<Answer> {
    gate(&app, &s.account).await?;
    let Some(p) = &app.cfg.presence else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "this control has no arugula.stream app"));
    };
    let team = a
        .room
        .strip_prefix("team:")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "a People room is team:<id>"))?;
    let t = app.cache.team(&*app.hub, team).await?;
    let Some(name) = t
        .as_ref()
        .filter(|t| t.role(&s.account).is_some())
        .and_then(|t| t.members.iter().find(|m| m.account == s.account).map(|m| m.name.clone()))
    else {
        return Err(err(StatusCode::NOT_FOUND, "no such team"));
    };
    let exp = arugula_e2e::now_ms() / 1000 + TTL_S;
    let token = room_token(p, &a.room, &s.account, &name, exp)
        .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "that room or account can't be an arugula.stream id"))?;
    Ok(Json(Answer { token, relay: p.relay.clone() }))
}

/// The claims, in the order arugula.stream's SDKs write them.
#[derive(Serialize)]
struct Claims<'a> {
    app: &'a str,
    room: &'a str,
    sub: &'a str,
    name: &'a str,
    exp: u64,
}

/// An arugula.stream room token (its PROTOCOL.md, "Room token"): an HS256
/// JWT over `{app, room, sub, name, exp}`. `None` when the room or `sub`
/// isn't an id the relay takes. A name with control characters loses them,
/// and is cut to 80.
fn room_token(p: &Presence, room: &str, sub: &str, name: &str, exp: u64) -> Option<String> {
    use hmac::{KeyInit, Mac};
    let id = |s: &str, at: bool| {
        (1..=128).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b) || (at && b == b'@'))
    };
    if !id(&p.app, false) || !id(room, false) || !id(sub, true) {
        return None;
    }
    let mut name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    if name.trim().is_empty() {
        name = sub.chars().take(80).collect();
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let claims = serde_json::to_vec(&Claims { app: &p.app, room, sub, name: &name, exp }).ok()?;
    let signed = format!("{}.{}", b64(br#"{"alg":"HS256","typ":"JWT"}"#), b64(&claims));
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(p.secret.as_bytes()).ok()?;
    mac.update(signed.as_bytes());
    Some(format!("{signed}.{}", b64(&mac.finalize().into_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acme() -> Presence {
        Presence { app: "acme".into(), secret: "acme-test-secret".into(), relay: String::new() }
    }

    /// arugula.stream's own vector ("basic", sdk/vectors.json): every SDK
    /// makes exactly this token, signed with a made-up test secret.
    const BASIC: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJhcHAiOiJhY21lIiwicm9vbSI6ImxvYmJ5Iiwic3ViIjoidV8xMjMiLCJuYW1lIjoiQWRhIiwiZXhwIjoxODAwMDAzNjAwfQ.aZUrFkhovb9WpzgZChQMpHVLpgVM66K6YvMSeQ3-IWc"; // gitleaks:allow

    #[test]
    fn a_token_is_byte_for_byte_arugula_streams() {
        assert_eq!(room_token(&acme(), "lobby", "u_123", "Ada", 1_800_003_600).as_deref(), Some(BASIC));
    }

    #[test]
    fn ids_the_relay_refuses_get_no_token() {
        assert!(room_token(&acme(), "team:a b", "u_1", "A", 1).is_none());
        assert!(room_token(&acme(), "team:1", "u/1", "A", 1).is_none());
        assert!(room_token(&acme(), "team:1", "", "A", 1).is_none());
        assert!(room_token(&acme(), "team:1", "jake@example.com", "A", 1).is_some());
    }
}
