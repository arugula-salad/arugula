//! Chat held by control (M122), over HTTP as pages and machines use it,
//! with no daemon running: S41's flow, then who sees what, mentions,
//! pushes, search, read marks, the live stream, and deletion.

use std::time::Duration;

use arugula_e2e::{DeviceKeys, now_ms, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    db::Team,
    push_notices::Phone,
    routing_wire::{Control, control, daemon, member, person, roster, session},
};

const TEAM: &str = "0123456789abcdef";

/// An open `GET /api/chat/live`.
struct Live {
    resp: reqwest::Response,
    buf: String,
    /// The last event id seen.
    last: Option<String>,
}

impl Live {
    async fn open(c: &Control, cookie: &str, last: Option<&str>) -> Live {
        let mut req = c.http.get(format!("{}/api/chat/live", c.base)).header("cookie", cookie);
        if let Some(l) = last {
            req = req.header("last-event-id", l);
        }
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), 200);
        Live { resp, buf: String::new(), last: last.map(str::to_owned) }
    }

    /// The next event's data, or none within a few seconds.
    async fn next(&mut self) -> Option<Value> {
        loop {
            if let Some(i) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..i + 2).collect();
                let mut data = None;
                for line in frame.lines() {
                    if let Some(id) = line.strip_prefix("id:") {
                        self.last = Some(id.trim().to_owned());
                    } else if let Some(d) = line.strip_prefix("data:") {
                        data = Some(serde_json::from_str(d.trim()).unwrap());
                    }
                }
                match data {
                    Some(d) => return Some(d),
                    None => continue,
                }
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.resp.chunk()).await.ok()?.ok()??;
            self.buf.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }

    /// Events until one of `type`, which it returns.
    async fn until(&mut self, ty: &str) -> Value {
        loop {
            let ev = self.next().await.unwrap_or_else(|| panic!("no {ty} event"));
            if ev["type"] == ty {
                return ev;
            }
        }
    }

    /// Nothing more within a moment.
    async fn quiet(&mut self) -> Option<Value> {
        tokio::time::timeout(Duration::from_millis(400), self.next()).await.ok().flatten()
    }
}

struct Rig {
    c: Control,
    jake: String,
    alex: String,
    sam: String,
    carol: String,
    alex_keys: DeviceKeys,
    jake_keys: DeviceKeys,
    carol_keys: DeviceKeys,
    box_a: DeviceKeys,
    box_b: DeviceKeys,
}

/// jake founds acme, with alex (an editor) and carol (a viewer); box-a is
/// jake's machine in acme, box-b alex's own; sam is in no team. Chat is on
/// for acme. Nothing dials in: no machine is up.
async fn rig() -> Rig {
    let c = control(|_| {}).await;
    let jake_keys = person(&c.app, "jake", "a1jake");
    let alex_keys = person(&c.app, "alex", "a2alex");
    let carol_keys = person(&c.app, "carol", "a4carol");
    person(&c.app, "sam", "a3sam");
    let r = roster(
        TEAM,
        1,
        vec![
            member("a1jake", &jake_keys, TeamRole::Owner),
            member("a2alex", &alex_keys, TeamRole::Editor),
            member("a4carol", &carol_keys, TeamRole::Viewer),
        ],
        &jake_keys,
    );
    let t = Team {
        id: TEAM.into(),
        name: "acme".into(),
        founder: "a1jake".into(),
        founder_root: jake_keys.id(),
        locked: false,
    };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    let box_a = daemon(&c.app, "a1jake", "box-a");
    c.app.db.set_daemon_team(&box_a.id(), TEAM).unwrap();
    let box_b = daemon(&c.app, "a2alex", "box-b");
    c.app.db.set_labs_flag(&format!("team:{TEAM}"), "chat", Some(true)).unwrap();
    Rig {
        jake: session(&c.app, "a1jake"),
        alex: session(&c.app, "a2alex"),
        sam: session(&c.app, "a3sam"),
        carol: session(&c.app, "a4carol"),
        c,
        alex_keys,
        jake_keys,
        carol_keys,
        box_a,
        box_b,
    }
}

impl Rig {
    async fn call(&self, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.c.as_person(who, method, path, body).await
    }

    async fn ok(&self, who: &str, method: &str, path: &str, body: Option<Value>) -> Value {
        let (st, v) = self.call(who, method, path, body).await;
        assert_eq!(st, 200, "{method} {path}: {v}");
        v
    }

    async fn post(&self, who: &str, room: &str, body: Value) -> Value {
        self.ok(who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(body)).await
    }

    async fn channel(&self, who: &str, name: &str) -> String {
        let v = self.ok(who, "POST", &format!("/api/chat/teams/{TEAM}/channels"), Some(json!({ "name": name }))).await;
        v["id"].as_str().unwrap().to_owned()
    }

    async fn rooms(&self, who: &str) -> Vec<Value> {
        self.ok(who, "GET", "/api/chat", None).await["rooms"].as_array().unwrap().clone()
    }

    async fn room_in_list(&self, who: &str, id: &str) -> Value {
        self.rooms(who).await.into_iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("{id} not listed"))
    }

    async fn messages(&self, who: &str, room: &str) -> Vec<Value> {
        self.ok(who, "GET", &format!("/api/chat/rooms/{room}/messages"), None).await["messages"]
            .as_array()
            .unwrap()
            .clone()
    }

    async fn search(&self, who: &str, q: &str) -> Vec<(String, i64)> {
        let path =
            format!("/api/chat/search?q={}", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>());
        self.ok(who, "GET", &path, None).await["found"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| (h["room"].as_str().unwrap().to_owned(), h["message"]["seq"].as_i64().unwrap()))
            .collect()
    }

    async fn readers(&self, machine: &DeviceKeys, body: Value) -> (u16, Value) {
        self.c.daemon_post(machine, arugula_control_wire::chat::CHAT_READERS, &body).await
    }
}

#[tokio::test]
async fn the_spike_flow_runs_with_every_machine_asleep() {
    let r = rig().await;
    let (mut alex_phone, _) = Phone::subscribe(&r.c.app, "a2alex", &r.alex_keys, 7).await;
    let (mut jake_phone, _) = Phone::subscribe(&r.c.app, "a1jake", &r.jake_keys, 8).await;

    // While it was up, box-a said who may read pane 2's thread: alex (not
    // carol). Then every machine sleeps.
    let (st, v) = r
        .readers(
            &r.box_a,
            json!({ "targets": [{ "kind": "pane", "target": "2", "readers": [{ "account": "a2alex" }] }] }),
        )
        .await;
    assert_eq!((st, v), (200, json!({ "stored": 1 })));
    assert!(!r.c.app.relay.online(&r.box_a.id()) && !r.c.app.relay.online(&r.box_b.id()), "no machine is up");

    // sam, signed in but with chat off, is refused as such; with it on, he
    // sees no rooms.
    let (st, v) = r.call(&r.sam, "GET", "/api/chat", None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("chat_off")));
    r.c.app.db.set_labs_flag("account:a3sam", "chat", Some(true)).unwrap();
    assert!(r.rooms(&r.sam).await.is_empty());

    let mut alex_live = Live::open(&r.c, &r.alex, None).await;
    assert!(alex_live.until("hello").await["stream"].is_string());

    // jake makes #general; a viewer can't make channels.
    let general = r.channel(&r.jake, "#General").await;
    let (st, _) =
        r.call(&r.carol, "POST", &format!("/api/chat/teams/{TEAM}/channels"), Some(json!({"name": "x"}))).await;
    assert_eq!(st, 403);
    assert_eq!(alex_live.until("room").await["room"], general.as_str());

    // jake mentions alex, quoting box-a's pane.
    let quote = "running 1 test\ntest mean ... FAILED";
    let posted = r
        .post(
            &r.jake,
            &general,
            json!({ "text": "@alex the mean test fails on box-a", "pane": { "machine": r.box_a.id(), "pane": "2" }, "quote": quote }),
        )
        .await;
    assert_eq!(posted["seq"], 1);
    assert_eq!(posted["mentions"], json!(["a2alex"]));
    assert_eq!(posted["pushed"], json!([{ "account": "a2alex", "why": "mention" }]));
    assert_eq!(alex_live.until("message").await, json!({ "type": "message", "room": general, "seq": 1 }));
    let n = alex_phone.next().await.expect("alex's phone hears of it");
    assert_eq!(n["title"], "jake mentioned you in #general");
    assert_eq!(n["body"], "@alex the mean test fails on box-a");
    assert_eq!((n["tag"].as_str(), &n["chat"]["room"]), (Some(format!("chat-{general}").as_str()), &json!(general)));
    assert!(jake_phone.quiet().await, "nobody is pushed for their own post");

    // alex reads it: box-a's chip opens for him (it's the team's).
    let ms = r.messages(&r.alex, &general).await;
    assert_eq!(ms[0]["pane"], json!({ "machine": r.box_a.id(), "machine_name": "box-a", "pane": "2", "opens": true }));
    assert_eq!((ms[0]["quote"].as_str(), ms[0]["name"].as_str()), (Some(quote), Some("jake")));
    // sam: no such room.
    assert_eq!(r.call(&r.sam, "GET", &format!("/api/chat/rooms/{general}/messages"), None).await.0, 404);
    assert_eq!(
        r.call(&r.sam, "POST", &format!("/api/chat/rooms/{general}/messages"), Some(json!({"text": "hi"}))).await.0,
        404
    );

    // alex quotes his own box-b: jake can't reach it, so his chip doesn't
    // open; nobody may name a pane they can't reach.
    let v = r
        .post(&r.alex, &general, json!({ "text": "deploy is fine here", "pane": { "machine": r.box_b.id(), "pane": "3" }, "quote": "deploy ok" }))
        .await;
    assert_eq!(v["seq"], 2);
    let (st, _) = r
        .call(
            &r.jake,
            "POST",
            &format!("/api/chat/rooms/{general}/messages"),
            Some(json!({ "text": "x", "pane": { "machine": r.box_b.id(), "pane": "3" } })),
        )
        .await;
    assert_eq!(st, 403);
    assert_eq!(r.messages(&r.jake, &general).await[1]["pane"]["opens"], false);
    assert_eq!(r.messages(&r.alex, &general).await[1]["pane"]["opens"], true);

    // A DM, alex to jake: a mention pushes; a second post within the minute
    // doesn't push again.
    let dm = r.ok(&r.alex, "POST", &format!("/api/chat/teams/{TEAM}/dms"), Some(json!({ "with": "a1jake" }))).await;
    assert_eq!((dm["kind"].as_str(), dm["with"]["name"].as_str()), (Some("dm"), Some("jake")));
    let dm = dm["id"].as_str().unwrap().to_owned();
    let again = r.ok(&r.jake, "POST", &format!("/api/chat/teams/{TEAM}/dms"), Some(json!({ "with": "a2alex" }))).await;
    assert_eq!(again["id"], dm.as_str(), "one DM per pair");
    let v = r.post(&r.alex, &dm, json!({ "text": "@jake can you look?" })).await;
    assert_eq!(v["pushed"], json!([{ "account": "a1jake", "why": "mention" }]));
    assert_eq!(jake_phone.next().await.unwrap()["title"], "alex mentioned you");
    assert_eq!(r.post(&r.alex, &dm, json!({ "text": "the other one too" })).await["pushed"], json!([]));
    assert_eq!(
        r.call(&r.carol, "GET", &format!("/api/chat/rooms/{dm}"), None).await.0,
        404,
        "a DM is its two people's"
    );

    // box-a's pane thread: alex is on its reader list; carol, a teammate
    // who isn't, and sam don't see it. jake, box-a's owner, always does.
    let open = json!({ "machine": r.box_a.id(), "kind": "pane", "target": "2" });
    let th = r.ok(&r.alex, "POST", "/api/chat/targets", Some(open.clone())).await;
    assert_eq!((th["name"].as_str(), th["machine_name"].as_str()), (Some("pane 2"), Some("box-a")));
    let th = th["id"].as_str().unwrap().to_owned();
    assert_eq!(r.call(&r.carol, "POST", "/api/chat/targets", Some(open.clone())).await.0, 404);
    assert_eq!(r.call(&r.sam, "POST", "/api/chat/targets", Some(open.clone())).await.0, 404);
    assert_eq!(r.ok(&r.jake, "POST", "/api/chat/targets", Some(open)).await["id"], th.as_str());
    r.post(&r.alex, &th, json!({ "text": "answered with box-a off" })).await;
    assert_eq!(r.call(&r.carol, "GET", &format!("/api/chat/rooms/{th}/messages"), None).await.0, 404);
    let people: Vec<String> = r.ok(&r.jake, "GET", &format!("/api/chat/rooms/{th}"), None).await["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["account"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(people, ["a1jake", "a2alex"], "whom a mention there reaches");

    // Unread and mention counts, and read marks.
    let g = r.room_in_list(&r.jake, &general).await;
    assert_eq!((g["unread"].as_u64(), g["mentions"].as_u64(), g["read"].as_i64()), (Some(1), Some(0), Some(1)));
    let d = r.room_in_list(&r.jake, &dm).await;
    assert_eq!((d["unread"].as_u64(), d["mentions"].as_u64()), (Some(2), Some(1)));
    assert_eq!(r.room_in_list(&r.jake, &th).await["unread"], 1);
    let g = r.ok(&r.jake, "POST", &format!("/api/chat/rooms/{general}/read"), Some(json!({ "seq": 99 }))).await;
    assert_eq!((g["read"].as_i64(), g["unread"].as_u64()), (Some(2), Some(0)), "a mark stops at the last message");
    let g = r.ok(&r.jake, "POST", &format!("/api/chat/rooms/{general}/read"), Some(json!({ "seq": 1 }))).await;
    assert_eq!(g["read"], 2, "and never goes back");
    assert_eq!(r.rooms(&r.carol).await.iter().map(|x| x["id"].clone()).collect::<Vec<_>>(), [json!(general)]);

    // Search: words, prefixes and quotes, only in rooms one sees.
    assert_eq!(r.search(&r.alex, "mean").await, [(general.clone(), 1)]);
    assert_eq!(r.search(&r.alex, "FAILED").await, [(general.clone(), 1)], "inside a quote");
    assert_eq!(r.search(&r.alex, "mea*").await, [(general.clone(), 1)]);
    assert!(r.search(&r.sam, "mean").await.is_empty());
    assert!(r.search(&r.carol, "answered").await.is_empty(), "carol doesn't read pane 2's thread");
    assert_eq!(r.search(&r.jake, "answered").await, [(th.clone(), 1)]);
    assert_eq!(r.search(&r.jake, "from:@alex in:#general").await, [(general.clone(), 2)]);
    assert_eq!(r.search(&r.jake, "has:quote fine").await, [(general.clone(), 2)]);
}

#[tokio::test]
async fn the_live_stream_resumes_where_it_left_off() {
    let r = rig().await;
    let general = r.channel(&r.jake, "general").await;
    let mut live = Live::open(&r.c, &r.alex, None).await;
    live.until("hello").await;
    r.post(&r.jake, &general, json!({ "text": "one" })).await;
    live.until("message").await;
    let last = live.last.clone().expect("events carry ids");
    drop(live);

    // While alex's page is away: a post he sees, and one in a room he
    // doesn't (jake's DM with carol).
    let dm = r.ok(&r.jake, "POST", &format!("/api/chat/teams/{TEAM}/dms"), Some(json!({ "with": "a4carol" }))).await;
    r.post(&r.jake, dm["id"].as_str().unwrap(), json!({ "text": "just us" })).await;
    r.post(&r.jake, &general, json!({ "text": "two" })).await;

    let mut live = Live::open(&r.c, &r.alex, Some(&last)).await;
    live.until("hello").await;
    assert_eq!(live.next().await.unwrap(), json!({ "type": "message", "room": general, "seq": 2 }));
    assert_eq!(live.quiet().await, None, "nothing of a room he doesn't see");

    // His read mark moving on another page reaches this one; nobody else's.
    r.ok(&r.alex, "POST", &format!("/api/chat/rooms/{general}/read"), Some(json!({ "seq": 2 }))).await;
    assert_eq!(live.next().await.unwrap(), json!({ "type": "read", "room": general, "seq": 2 }));
    r.ok(&r.jake, "POST", &format!("/api/chat/rooms/{general}/read"), Some(json!({ "seq": 2 }))).await;
    assert_eq!(live.quiet().await, None);

    // An id from before a restart (another epoch): resync.
    let mut live = Live::open(&r.c, &r.alex, Some("0-5")).await;
    live.until("hello").await;
    assert_eq!(live.next().await.unwrap(), json!({ "type": "resync" }));
}

#[tokio::test]
async fn pushes_skip_open_muted_and_unwatched_rooms() {
    let r = rig().await;
    let (mut phone, _) = Phone::subscribe(&r.c.app, "a2alex", &r.alex_keys, 9).await;
    let ops = r.channel(&r.jake, "ops").await;
    let mut live = Live::open(&r.c, &r.alex, None).await;
    let stream = live.until("hello").await["stream"].clone();

    // alex has #ops open: a mention there isn't pushed.
    r.ok(&r.alex, "POST", "/api/chat/live/open", Some(json!({ "stream": stream, "room": ops }))).await;
    assert_eq!(r.post(&r.jake, &ops, json!({ "text": "@alex look" })).await["pushed"], json!([]));
    // Someone else can't say what his stream has open.
    let (st, _) =
        r.call(&r.carol, "POST", "/api/chat/live/open", Some(json!({ "stream": stream, "room": null }))).await;
    assert_eq!(st, 404);

    // Muted: no push either.
    let quiet = r.channel(&r.jake, "quiet").await;
    r.ok(&r.alex, "POST", &format!("/api/chat/rooms/{quiet}/mute"), Some(json!({ "muted": true }))).await;
    assert_eq!(r.post(&r.jake, &quiet, json!({ "text": "@alex shh" })).await["pushed"], json!([]));

    // A mention of nobody in the room reaches nobody, and says so; a plain
    // channel message pushes nobody.
    let v = r.post(&r.jake, &quiet, json!({ "text": "@sam @Alex hello" })).await;
    assert_eq!((v["mentions"].clone(), v["unreached"].clone()), (json!(["a2alex"]), json!(["sam"])));
    assert_eq!(r.post(&r.jake, &ops, json!({ "text": "no names" })).await["mentions"], json!([]));
    assert!(phone.quiet().await);

    // With the room closed, a mention pushes: who, where, and the first
    // 200 characters.
    r.ok(&r.alex, "POST", "/api/chat/live/open", Some(json!({ "stream": stream, "room": null }))).await;
    let long = format!("@alex {}", "x".repeat(300));
    assert_eq!(r.post(&r.jake, &ops, json!({ "text": long })).await["pushed"][0]["account"], "a2alex");
    let n = phone.next().await.unwrap();
    assert_eq!(n["title"], "jake mentioned you in #ops");
    assert_eq!(n["body"].as_str().unwrap().chars().count(), 200);
}

#[tokio::test]
async fn reader_lists_decide_who_reads_a_panes_thread() {
    let r = rig().await;
    let open = |t: &str| json!({ "machine": r.box_a.id(), "kind": "session", "target": t });
    // Before box-a has said: its owner and the team's owners (jake is
    // both), not other teammates.
    let th = r.ok(&r.jake, "POST", "/api/chat/targets", Some(open("1"))).await["id"].as_str().unwrap().to_owned();
    assert_eq!(r.call(&r.alex, "GET", &format!("/api/chat/rooms/{th}"), None).await.0, 404);

    // It says carol may read from now on: she sees what's posted after.
    r.post(&r.jake, &th, json!({ "text": "before carol" })).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let from = now_ms();
    let (st, _) = r
        .readers(&r.box_a, json!({ "targets": [{ "kind": "session", "target": "1", "readers": [{ "account": "a4carol", "from": from }] }] }))
        .await;
    assert_eq!(st, 200);
    r.post(&r.jake, &th, json!({ "text": "after carol" })).await;
    let seen: Vec<String> =
        r.messages(&r.carol, &th).await.iter().map(|m| m["text"].as_str().unwrap().to_owned()).collect();
    assert_eq!(seen, ["after carol"]);
    assert_eq!(r.room_in_list(&r.carol, &th).await["unread"], 1);
    assert_eq!(r.search(&r.carol, "before").await, []);
    assert_eq!(r.call(&r.alex, "GET", &format!("/api/chat/rooms/{th}"), None).await.0, 404, "alex isn't listed");

    // A list names only accounts control would route there anyway: sam,
    // in no team with box-a, isn't let in by being listed.
    r.c.app.db.set_labs_flag("account:a3sam", "chat", Some(true)).unwrap();
    r.readers(
        &r.box_a,
        json!({ "targets": [{ "kind": "session", "target": "1", "readers": [{ "account": "a3sam" }] }] }),
    )
    .await;
    assert_eq!(r.call(&r.sam, "GET", &format!("/api/chat/rooms/{th}"), None).await.0, 404);
    assert_eq!(
        r.call(&r.carol, "GET", &format!("/api/chat/rooms/{th}"), None).await.0,
        404,
        "the new list replaced hers"
    );

    // A machine whose account and team have chat off isn't heard.
    let sams_box = daemon(&r.c.app, "a3sam", "sams-box");
    r.c.app.db.set_labs_flag("account:a3sam", "chat", Some(false)).unwrap();
    let (st, v) = r.readers(&sams_box, json!({ "targets": [] })).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("chat_off")));
    let _ = &r.carol_keys;
}

#[tokio::test]
async fn messages_are_edited_and_deleted_by_whom_they_may_be() {
    let r = rig().await;
    let general = r.channel(&r.jake, "general").await;
    r.post(&r.alex, &general, json!({ "text": "@jake typo hre", "quote": "secret line" })).await;
    let path = format!("/api/chat/rooms/{general}/messages/1");
    assert_eq!(r.call(&r.carol, "PATCH", &path, Some(json!({ "text": "mine now" }))).await.0, 403);
    let m = r.ok(&r.alex, "PATCH", &path, Some(json!({ "text": "typo here, no names" }))).await;
    assert_eq!((m["text"].as_str(), m["mentions"].clone()), (Some("typo here, no names"), json!([])));
    assert!(m["edited"].is_u64());
    assert_eq!(r.search(&r.jake, "hre").await, []);

    // carol (a viewer) can't delete it; jake, an owner, can.
    assert_eq!(r.call(&r.carol, "DELETE", &path, None).await.0, 403);
    let m = r.ok(&r.jake, "DELETE", &path, None).await;
    assert_eq!((m["text"].as_str(), m["quote"].clone(), m["deleted"].is_u64()), (Some(""), Value::Null, true));
    assert_eq!(r.search(&r.jake, "secret").await, []);
    assert_eq!(r.post(&r.alex, &general, json!({ "text": "next" })).await["seq"], 2, "its number stays taken");

    // An archived channel is read, not written; only owners archive.
    let change = format!("/api/chat/rooms/{general}");
    assert_eq!(r.call(&r.alex, "PATCH", &change, Some(json!({ "archived": true }))).await.0, 403);
    let g = r.ok(&r.jake, "PATCH", &change, Some(json!({ "archived": true, "name": "old-general" }))).await;
    assert_eq!(
        (g["archived"].as_bool(), g["writable"].as_bool(), g["name"].as_str()),
        (Some(true), Some(false), Some("old-general"))
    );
    assert_eq!(r.call(&r.alex, "POST", &format!("{change}/messages"), Some(json!({ "text": "hi" }))).await.0, 409);
    assert_eq!(r.messages(&r.alex, &general).await.len(), 2);
}

#[tokio::test]
async fn leaving_the_team_and_deleting_the_account_take_its_chat() {
    let r = rig().await;
    let general = r.channel(&r.jake, "general").await;
    r.post(&r.alex, &general, json!({ "text": "alex was here", "quote": "a quoted line" })).await;
    r.post(&r.jake, &general, json!({ "text": "@alex thanks" })).await;
    let dm = r.ok(&r.jake, "POST", &format!("/api/chat/teams/{TEAM}/dms"), Some(json!({ "with": "a2alex" }))).await;
    let dm = dm["id"].as_str().unwrap().to_owned();
    r.post(&r.alex, &dm, json!({ "text": "between us" })).await;
    let bth = r
        .ok(
            &r.alex,
            "POST",
            "/api/chat/targets",
            Some(json!({ "machine": r.box_b.id(), "kind": "pane", "target": "1" })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    r.post(&r.alex, &bth, json!({ "text": "on my own box" })).await;

    // alex leaves acme: the DM is gone for him and read-only for jake; the
    // team's channels are gone for him; his messages stay.
    let v2 = roster(
        TEAM,
        2,
        vec![member("a1jake", &r.jake_keys, TeamRole::Owner), member("a4carol", &r.carol_keys, TeamRole::Viewer)],
        &r.jake_keys,
    );
    r.c.app.db.add_roster(TEAM, 2, &serde_json::to_string(&v2).unwrap()).unwrap();
    r.c.app.db.set_labs_flag("account:a2alex", "chat", Some(true)).unwrap();
    assert_eq!(r.call(&r.alex, "GET", &format!("/api/chat/rooms/{dm}"), None).await.0, 404);
    assert_eq!(r.call(&r.alex, "GET", &format!("/api/chat/rooms/{general}"), None).await.0, 404);
    assert_eq!(r.room_in_list(&r.jake, &dm).await["writable"], false);
    assert_eq!(
        r.call(&r.jake, "POST", &format!("/api/chat/rooms/{dm}/messages"), Some(json!({ "text": "hello?" }))).await.0,
        409
    );
    assert_eq!(r.messages(&r.jake, &general).await[0]["text"], "alex was here");

    // alex deletes his account: his DMs and his machines' threads go, and
    // his messages elsewhere are blank and nobody's.
    let (st, v) = r.call(&r.alex, "POST", "/api/me/delete", Some(json!({ "confirm": "alex" }))).await;
    assert_eq!(st, 200, "{v}");
    assert!(r.c.app.db.chat_room(&dm).unwrap().is_none());
    assert!(r.c.app.db.chat_room(&bth).unwrap().is_none());
    let ms = r.messages(&r.jake, &general).await;
    assert_eq!(
        (ms[0]["author"].as_str(), ms[0]["name"].as_str(), ms[0]["text"].as_str(), ms[0]["deleted"].is_u64()),
        (Some(""), Some(""), Some(""), true)
    );
    assert_eq!(ms[1]["mentions"], json!([]));
    assert_eq!(r.search(&r.jake, "quoted").await, []);
    let left: Vec<String> = r.c.app.db.mentions("a2alex").into_iter().filter(|t| t.starts_with("chat")).collect();
    assert!(left.is_empty(), "{left:?}");
}
