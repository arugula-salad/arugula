//! Telling a room when a grant first lets an agent in (#122). The room
//! header's chip lists the agents that can read it, but a grant given today
//! changes who can read yesterday's messages, so the room gets one line of
//! its own the moment it happens: "alecraso's agents on geek can now read
//! and post here". The line is the platform's, with no author and
//! [`SYSTEM_VIA`] for `via`; it isn't unread for anyone and pushes nobody.
//!
//! Nothing is remembered: [`give`](crate::grants::give) looks at who could
//! reach each room before the grant and after, and says what's new
//! (a machine that wasn't there, or a power it hadn't). Giving what's
//! already given says nothing, and a grant over a project says it in the
//! team's channels only, not in every issue's thread.

use std::collections::{BTreeMap, HashMap};

use arugula_control_wire::chat::{ChatLive, SYSTEM_VIA};
use arugula_e2e::now_ms;

use super::{Facts, room_agents};
use crate::{
    Platform,
    db::{
        chat::{NewMessage, Room},
        projects::{THREADS_POST, THREADS_READ},
    },
};

/// The most rooms one grant says anything in.
const MAX_ROOMS: usize = 200;

/// The powers each machine's agents have in each room: by room, then machine.
pub(crate) type Reach = HashMap<String, BTreeMap<String, Reached>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reached {
    pub owner: String,
    pub owner_name: String,
    pub machine_name: String,
    pub powers: Vec<String>,
}

/// The rooms a grant over `(kind, id)` reaches.
pub(crate) fn rooms(app: &Platform, scope: (&str, &str)) -> anyhow::Result<Vec<Room>> {
    let mut rooms = app.db.chat_rooms_reached(scope.0, scope.1)?;
    rooms.truncate(MAX_ROOMS);
    Ok(rooms)
}

/// Who reaches `rooms` now.
pub(crate) async fn reach(app: &Platform, rooms: &[Room]) -> anyhow::Result<Reach> {
    let mut out = Reach::new();
    for room in rooms {
        let facts = Facts::of(app, None, &[room]).await?;
        let agents = room_agents(app, &facts, room).await?;
        let by = out.entry(room.id.clone()).or_default();
        for a in agents {
            by.insert(
                a.machine,
                Reached { owner: a.owner, owner_name: a.owner_name, machine_name: a.machine_name, powers: a.powers },
            );
        }
    }
    Ok(out)
}

/// What an agent may do in a room, in words; `None` when it's neither.
fn can(powers: &[String]) -> Option<&'static str> {
    let has = |p: &str| powers.iter().any(|x| x == p);
    match (has(THREADS_READ), has(THREADS_POST)) {
        (true, true) => Some("read and post"),
        (true, false) => Some("read"),
        (false, true) => Some("post"),
        (false, false) => None,
    }
}

/// The lines for one room: for each owner and what their new machines may
/// do, one. Pure, so the wording is tested.
fn lines(before: Option<&BTreeMap<String, Reached>>, after: &BTreeMap<String, Reached>) -> Vec<String> {
    let mut groups: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    let mut who: BTreeMap<&str, &str> = BTreeMap::new();
    for (machine, now) in after {
        let new = match before.and_then(|b| b.get(machine)) {
            None => true,
            Some(was) => now.powers.iter().any(|p| !was.powers.contains(p)),
        };
        let Some(verb) = can(&now.powers).filter(|_| new) else { continue };
        groups.entry((now.owner.as_str(), verb)).or_default().push(now.machine_name.as_str());
        who.insert(now.owner.as_str(), now.owner_name.as_str());
    }
    groups
        .into_iter()
        .map(|((owner, verb), machines)| {
            let name = who.get(owner).filter(|n| !n.is_empty()).copied().unwrap_or("someone");
            let on = match machines.as_slice() {
                [one] => (*one).to_owned(),
                [init @ .., last] => format!("{} and {last}", init.join(", ")),
                [] => String::new(),
            };
            format!("{name}'s agents on {on} can now {verb} here")
        })
        .collect()
}

/// Say, in each of `rooms`, what `before` didn't have. A failure is logged:
/// the grant is given either way.
pub(crate) async fn announce(app: &Platform, rooms: &[Room], before: &Reach) {
    let after = match reach(app, rooms).await {
        Ok(a) => a,
        Err(e) => return tracing::warn!(error = %e, "chat: reading who reaches a room after a grant"),
    };
    for room in rooms {
        let Some(now) = after.get(&room.id) else { continue };
        for text in lines(before.get(&room.id), now) {
            let posted = app.db.add_chat_message(
                &room.id,
                &NewMessage {
                    author: "",
                    at: now_ms(),
                    text: &text,
                    quote: None,
                    ref_machine: None,
                    ref_pane: None,
                    mentions: &[],
                    agent: None,
                    via: Some(SYSTEM_VIA),
                },
            );
            match posted {
                Ok(seq) => app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq }, None),
                Err(e) => tracing::warn!(error = %e, "chat: saying a grant reached a room"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(owner: &str, machine: &str, powers: &[&str]) -> (String, Reached) {
        (
            machine.to_owned(),
            Reached {
                owner: owner.into(),
                owner_name: owner.to_uppercase(),
                machine_name: format!("{machine}.local"),
                powers: powers.iter().map(|p| (*p).to_owned()).collect(),
            },
        )
    }

    #[test]
    fn a_new_machine_or_a_new_power_is_said_once() {
        let before: BTreeMap<_, _> = [at("a", "m1", &[THREADS_READ])].into();
        let same = lines(Some(&before), &before);
        assert!(same.is_empty(), "{same:?}");
        let after: BTreeMap<_, _> =
            [at("a", "m1", &[THREADS_READ, THREADS_POST]), at("a", "m2", &[THREADS_READ, THREADS_POST])].into();
        assert_eq!(lines(Some(&before), &after), ["A's agents on m1.local and m2.local can now read and post here"]);
        assert_eq!(lines(None, &[at("b", "m3", &[THREADS_POST])].into()), ["B's agents on m3.local can now post here"]);
        // Only a grant that gives nothing to do here is silent.
        assert!(lines(None, &[at("b", "m3", &["issues.write"])].into()).is_empty());
    }

    /// A grant over #general's project says so in #general, once, as a line
    /// with no author that nobody has unread; the same grant again, or one
    /// over something else, says nothing.
    #[tokio::test]
    async fn a_new_grant_says_so_in_the_room_once() {
        use crate::{
            agents_tests::{give, seeded},
            chat_tests::call,
        };
        use serde_json::json;
        let (_hub, app, base, t, _u) = seeded().await;
        let lines = |v: &serde_json::Value| -> Vec<(String, String, String)> {
            v["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|m| m["via"] == SYSTEM_VIA)
                .map(|m| (m["author"].to_string(), m["text"].as_str().unwrap().into(), m["name"].to_string()))
                .collect()
        };
        let scope = json!({ "kind": "project", "id": t });
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;
        let (_, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general/messages", None).await;
        assert_eq!(lines(&v).len(), 1, "{v}");
        assert_eq!(lines(&v)[0].1, "a's agents on geek-box can now read here");
        // Giving it again changes nothing; posting is new.
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, scope.clone())).await;
        // Not a thread power: nothing for the room to say.
        call(&base, "a", "POST", "/api/grants", give("geek", "issues.write", scope)).await;
        let (_, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general/messages", None).await;
        let said: Vec<_> = lines(&v).into_iter().map(|l| l.1).collect();
        assert_eq!(
            said,
            ["a's agents on geek-box can now read here", "a's agents on geek-box can now read and post here"]
        );
        // Nobody has it unread, and it isn't anyone's.
        let rooms = call(&base, "b", "GET", "/api/chat", None).await.1;
        let general = rooms["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap().clone();
        assert_eq!(general["unread"], 0, "{general}");
        // Over a's DMs it's said in a's DMs, not in b and x's.
        call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "account" }))).await;
        let (_, dm) = call(&base, "b", "GET", "/api/chat/rooms/dm_ab/messages", None).await;
        assert_eq!(lines(&dm).len(), 1, "{dm}");
        let (_, other) = call(&base, "b", "GET", "/api/chat/rooms/dm_bx/messages", None).await;
        assert!(lines(&other).is_empty(), "{other}");
        let _ = app;
    }
}
