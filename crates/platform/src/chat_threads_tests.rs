//! Reply threads (#106): what the room's list holds, who sees a thread,
//! its own read marks and unread, following, pushes to followers, search,
//! the live stream, a deleted parent, and an agent's reply.

use std::time::Duration;

use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{call, hears, open_live},
    db::projects::{THREADS_POST, THREADS_READ},
};

const ROOM: &str = "/api/chat/rooms/ch_general";

async fn say(base: &str, who: &str, text: &str) -> i64 {
    let (st, v) = call(base, who, "POST", &format!("{ROOM}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// A reply in the thread of `parent`; `also` sends it to the channel too.
async fn reply(base: &str, who: &str, parent: i64, text: &str, also: bool) -> Value {
    let (st, v) = call(
        base,
        who,
        "POST",
        &format!("{ROOM}/messages"),
        Some(json!({ "text": text, "parent": parent, "also": also })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    v
}

async fn list(base: &str, who: &str) -> Vec<Value> {
    let (st, v) = call(base, who, "GET", &format!("{ROOM}/messages"), None).await;
    assert_eq!(st, 200, "{v}");
    v["messages"].as_array().unwrap().clone()
}

async fn thread(base: &str, who: &str, parent: i64) -> Value {
    let (st, v) = call(base, who, "GET", &format!("{ROOM}/messages?thread={parent}"), None).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// The `ch_general` entry of `GET /api/chat` as `who` reads it, and the
/// Threads view's badge.
async fn room_of(base: &str, who: &str) -> (Value, Value) {
    let (st, v) = call(base, who, "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let room = v["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap().clone();
    (room, json!({ "unread": v["threads_unread"], "mentions": v["threads_mentions"] }))
}

async fn follow(base: &str, who: &str, parent: i64, on: bool) -> Value {
    let (st, v) =
        call(base, who, "POST", &format!("{ROOM}/threads/{parent}/follow"), Some(json!({ "following": on }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

async fn pushed(hub: &crate::chat_tests::TestHub) -> Vec<(String, Value)> {
    // They go out in the background.
    tokio::time::sleep(Duration::from_millis(150)).await;
    hub.pushes().into_iter().map(|(to, msg, _)| (to, msg)).collect()
}

/// A reply stays out of the room's list, and the parent says it has one;
/// sent to the channel too, it is in both. A thread is one deep, and only
/// a message that is there takes replies.
#[tokio::test]
async fn replies_stay_out_of_the_list_unless_also_sent() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "deploy at five?").await;
    let r1 = reply(&base, "b", p, "yes, after the freeze", false).await["seq"].as_i64().unwrap();
    let main = list(&base, "a").await;
    assert_eq!(main.iter().map(|m| m["seq"].as_i64().unwrap()).collect::<Vec<_>>(), [p], "{main:?}");
    let t = &main[0]["thread"];
    assert_eq!((&t["replies"], &t["repliers"][0]["account"]), (&json!(1), &json!("b")), "{t}");

    let r2 = reply(&base, "a", p, "also for the channel", true).await["seq"].as_i64().unwrap();
    let main = list(&base, "b").await;
    assert_eq!(main.iter().map(|m| m["seq"].as_i64().unwrap()).collect::<Vec<_>>(), [p, r2]);
    assert_eq!((&main[1]["parent"], &main[1]["also"]), (&json!(p), &json!(true)));
    assert_eq!(main[0]["thread"]["replies"], 2);
    assert_eq!(main[0]["thread"]["repliers"][0]["account"], "a", "the newest first");

    let v = thread(&base, "b", p).await;
    let seqs: Vec<i64> = v["messages"].as_array().unwrap().iter().map(|m| m["seq"].as_i64().unwrap()).collect();
    assert_eq!((seqs, &v["parent"]["seq"], &v["parent"]["text"]), (vec![r1, r2], &json!(p), &json!("deploy at five?")));
    let after = call(&base, "b", "GET", &format!("{ROOM}/messages?thread={p}&after={r1}"), None).await.1;
    assert_eq!(after["messages"].as_array().unwrap().len(), 1);

    // One deep; and only under a message that is there.
    let (st, v) =
        call(&base, "a", "POST", &format!("{ROOM}/messages"), Some(json!({ "text": "x", "parent": r1 }))).await;
    assert_eq!(st, 400, "{v}");
    let (st, _) =
        call(&base, "a", "POST", &format!("{ROOM}/messages"), Some(json!({ "text": "x", "parent": 99 }))).await;
    assert_eq!(st, 404);
    let (st, _) = call(&base, "a", "GET", &format!("{ROOM}/messages?thread={r1}"), None).await;
    assert_eq!(st, 400, "a reply starts no thread");
}

/// Who sees a thread is who sees its room: a person outside the team, or
/// outside a DM, finds neither the thread nor a way to follow or read it.
#[tokio::test]
async fn a_thread_is_seen_by_the_room() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "private-ish").await;
    reply(&base, "b", p, "noted", false).await;
    // c isn't in the team.
    let (st, _) = call(&base, "c", "GET", &format!("{ROOM}/messages?thread={p}"), None).await;
    assert_ne!(st, 200);
    let (st, _) =
        call(&base, "c", "POST", &format!("{ROOM}/threads/{p}/follow"), Some(json!({ "following": true }))).await;
    assert_ne!(st, 200);
    let (st, _) = call(&base, "c", "POST", &format!("{ROOM}/threads/{p}/read"), Some(json!({ "seq": 5 }))).await;
    assert_ne!(st, 200);
    let (st, _) =
        call(&base, "c", "POST", &format!("{ROOM}/messages"), Some(json!({ "text": "hi", "parent": p }))).await;
    assert_ne!(st, 200);

    // A DM's thread is the two people's.
    let dm = "/api/chat/rooms/dm_ab";
    let (_, m) = call(&base, "a", "POST", &format!("{dm}/messages"), Some(json!({ "text": "psst" }))).await;
    let dp = m["seq"].as_i64().unwrap();
    let (st, _) =
        call(&base, "b", "POST", &format!("{dm}/messages"), Some(json!({ "text": "hm", "parent": dp }))).await;
    assert_eq!(st, 200);
    let (st, _) = call(&base, "x", "GET", &format!("{dm}/messages?thread={dp}"), None).await;
    assert_eq!(st, 404, "x isn't in a and b's DM");
    // Nor in their Threads view.
    let (_, v) = call(&base, "x", "GET", "/api/chat/threads", None).await;
    assert_eq!(v["threads"], json!([]));
}

/// A thread has its own mark and count, apart from the room's: a reply
/// isn't an unread message of the room (unless also sent to the channel),
/// and writing one doesn't read the room.
#[tokio::test]
async fn a_thread_has_its_own_read_mark_and_unread() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "plan?").await;
    let r1 = reply(&base, "b", p, "one", false).await["seq"].as_i64().unwrap();
    let r2 = reply(&base, "b", p, "two", false).await["seq"].as_i64().unwrap();

    // a wrote the parent, so follows: two unread there, none in the room.
    let (room, badge) = room_of(&base, "a").await;
    assert_eq!((&room["unread"], &badge["unread"]), (&json!(0), &json!(2)), "{room} {badge}");
    let main = list(&base, "a").await;
    let t = &main[0]["thread"];
    assert_eq!(
        (&t["following"], &t["unread"], &t["read"]),
        (&json!(true), &json!(2), &json!(p)),
        "from the first reply on"
    );

    // b replied without reading the room: the parent is still unread there,
    // and b's own replies aren't unread in the thread.
    let (room, badge) = room_of(&base, "b").await;
    assert_eq!((&room["unread"], &room["read"], &badge["unread"]), (&json!(1), &json!(0), &json!(0)), "{room}");

    // Reading is forward only, and answers the parent.
    let (st, m) = call(&base, "a", "POST", &format!("{ROOM}/threads/{p}/read"), Some(json!({ "seq": r1 }))).await;
    assert_eq!((st, &m["thread"]["unread"]), (200, &json!(1)), "{m}");
    let (_, m) = call(&base, "a", "POST", &format!("{ROOM}/threads/{p}/read"), Some(json!({ "seq": 1 }))).await;
    assert_eq!(m["thread"]["read"], r1, "never back");
    let (_, m) = call(&base, "a", "POST", &format!("{ROOM}/threads/{p}/read"), Some(json!({ "seq": r2 + 50 }))).await;
    assert_eq!(
        (&m["thread"]["unread"], &m["thread"]["read"]),
        (&json!(0), &json!(r2)),
        "no further than the room's newest"
    );
    assert_eq!(room_of(&base, "a").await.1["unread"], 0);

    // Sent to the channel too, a reply is the room's unread as well.
    reply(&base, "b", p, "three", true).await;
    let (room, badge) = room_of(&base, "a").await;
    assert_eq!((&room["unread"], &badge["unread"]), (&json!(1), &json!(1)), "{room}");

    // And the Threads view lists it, with that count.
    let (_, v) = call(&base, "a", "GET", "/api/chat/threads", None).await;
    assert_eq!(v["unread"], 1);
    assert_eq!((&v["threads"][0]["room"], &v["threads"][0]["parent"]["seq"]), (&json!("ch_general"), &json!(p)));
    assert_eq!(v["threads"][0]["latest"]["text"], "three");
    assert_eq!(v["threads"][0]["parent"]["thread"]["unread"], 1);
}

/// The parent's author and whoever replies follow; anyone follows or stops;
/// following begins from now; a reply makes its writer follow again, but
/// others' replies don't undo an unfollow.
#[tokio::test]
async fn followers_are_the_author_the_repliers_and_whoever_asks() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "thoughts?").await;
    reply(&base, "b", p, "first", false).await;
    let following = |v: &Value| v["thread"]["following"].as_bool().unwrap();
    assert!(following(&list(&base, "a").await[0]) && following(&list(&base, "b").await[0]));
    assert!(!following(&list(&base, "x").await[0]), "x was not in it");

    // x follows: what's there is read; the next reply is unread.
    let m = follow(&base, "x", p, true).await;
    assert_eq!((&m["thread"]["following"], &m["thread"]["unread"]), (&json!(true), &json!(0)), "{m}");
    reply(&base, "a", p, "second", false).await;
    assert_eq!(list(&base, "x").await[0]["thread"]["unread"], 1);
    assert_eq!(room_of(&base, "x").await.1["unread"], 1);

    // a stops following: no unread there, and b's reply doesn't bring a back.
    let m = follow(&base, "a", p, false).await;
    assert_eq!((&m["thread"]["following"], &m["thread"]["unread"]), (&json!(false), &json!(0)));
    reply(&base, "b", p, "third", false).await;
    assert!(!following(&list(&base, "a").await[0]));
    assert_eq!(room_of(&base, "a").await.1["unread"], 0);
    let (_, v) = call(&base, "a", "GET", "/api/chat/threads", None).await;
    assert_eq!(v["threads"], json!([]));
    // But replying makes a a follower again.
    reply(&base, "a", p, "fourth", false).await;
    assert!(following(&list(&base, "a").await[0]));
}

/// Followers hear replies at their level for threads; others don't; a
/// mention in a reply is pushed as anywhere and brings that person in.
#[tokio::test]
async fn followers_are_pushed_for_replies_and_the_mentioned_follow() {
    let (hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "b", "who has the key?").await;

    // b follows (the author); a's reply reaches b, as a reply in a thread.
    let v = reply(&base, "a", p, "I do", false).await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "thread" }]), "{v}");
    let pushes = pushed(&hub).await;
    assert_eq!(pushes.len(), 1, "{pushes:?}");
    assert_eq!(pushes[0].0, "b");
    assert_eq!(pushes[0].1["title"], "a replied in a thread in #ch_general");
    assert_eq!(
        (&pushes[0].1["chat"]["parent"], &pushes[0].1["tag"]),
        (&json!(p), &json!(format!("chat-ch_general-t{p}")))
    );

    // x isn't in it. A mention brings x in, pushed as a mention, not twice.
    let v = reply(&base, "a", p, "@x can bring it", false).await;
    // b's second push inside the minute is held back by the push gap.
    assert_eq!(v["pushed"], json!([{ "account": "x", "why": "mention" }]), "{v}");
    assert_eq!(list(&base, "x").await[0]["thread"]["following"], true);
    assert_eq!(list(&base, "x").await[0]["thread"]["unread"], 1, "from the mention on");

    // Their level for followed threads: nothing silences them (in another
    // thread, which has no push gap of its own yet).
    let (st, _) = call(&base, "b", "POST", "/api/notify/prefs", Some(json!({ "threads": "nothing" }))).await;
    assert_eq!(st, 200);
    let p2 = say(&base, "b", "and another thing").await;
    let v = reply(&base, "a", p2, "any news?", false).await;
    assert_eq!(v["pushed"], json!([]), "{v}");
}

/// A reply open in the thread isn't pushed to the person reading it, but
/// the room being open isn't enough.
#[tokio::test]
async fn nobody_is_pushed_for_a_thread_they_have_open() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "b", "open question").await;
    let p2 = say(&base, "b", "another question").await;
    let live = open_live(&base, "b", None).await;
    hears(&live, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let stream = {
        let h = live.lock().unwrap().clone();
        let at = h.find("\"stream\":\"").unwrap() + 10;
        h[at..].split('"').next().unwrap().to_owned()
    };
    // The room open, the thread not: still pushed.
    let (st, _) =
        call(&base, "b", "POST", "/api/chat/live/open", Some(json!({ "stream": stream, "room": "ch_general" }))).await;
    assert_eq!(st, 200);
    let v = reply(&base, "a", p, "one", false).await;
    assert_eq!(v["pushed"], json!([{ "account": "b", "why": "thread" }]), "{v}");
    // With the thread open: not (another thread, the first is in its push gap).
    let (st, _) = call(
        &base,
        "b",
        "POST",
        "/api/chat/live/open",
        Some(json!({ "stream": stream, "room": "ch_general", "thread": p2 })),
    )
    .await;
    assert_eq!(st, 200);
    let v = reply(&base, "a", p2, "two", false).await;
    assert_eq!(v["pushed"], json!([]), "{v}");
}

/// Search finds a reply, which says which thread it is in.
#[tokio::test]
async fn search_finds_replies() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "rollout").await;
    let r = reply(&base, "b", p, "the canary looks greedy", false).await["seq"].as_i64().unwrap();
    let (st, v) = call(&base, "a", "GET", "/api/chat/search?q=canary", None).await;
    assert_eq!(st, 200, "{v}");
    let hit = &v["found"][0];
    assert_eq!(
        (&hit["room"], &hit["message"]["seq"], &hit["message"]["parent"]),
        (&json!("ch_general"), &json!(r), &json!(p))
    );
}

/// A new reply is a `reply` event, not a new message of the room; sent to
/// the channel too it is both. Edits and deletes of a reply are replies
/// events as well, and a person's own mark is told to their other pages.
#[tokio::test]
async fn the_stream_says_reply() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "hello").await;
    let live = open_live(&base, "b", None).await;
    hears(&live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    let r1 = reply(&base, "a", p, "in the thread", false).await["seq"].as_i64().unwrap();
    hears(
        &live,
        &format!("\"type\":\"reply\",\"room\":\"ch_general\",\"parent\":{p},\"seq\":{r1}"),
        Duration::from_secs(5),
    )
    .await;
    let said = live.lock().unwrap().clone();
    assert!(!said.contains(&format!("\"type\":\"message\",\"room\":\"ch_general\",\"seq\":{r1}")), "{said}");

    let r2 = reply(&base, "a", p, "and in the channel", true).await["seq"].as_i64().unwrap();
    hears(&live, &format!("\"type\":\"message\",\"room\":\"ch_general\",\"seq\":{r2}"), Duration::from_secs(5)).await;

    let (st, _) = call(&base, "a", "PATCH", &format!("{ROOM}/messages/{r1}"), Some(json!({ "text": "edited" }))).await;
    assert_eq!(st, 200);
    let heard = || live.lock().unwrap().matches(&format!("\"parent\":{p},\"seq\":{r1}")).count();
    for _ in 0..100 {
        if heard() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(heard(), 2, "the reply and its edit: {}", live.lock().unwrap());

    // b's mark: told to b only.
    let mine = open_live(&base, "b", None).await;
    hears(&mine, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let other = open_live(&base, "a", None).await;
    hears(&other, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    // Following, and reading a reply after it, are told to b's pages.
    follow(&base, "b", p, true).await;
    hears(&mine, "\"type\":\"threadread\"", Duration::from_secs(5)).await;
    let r3 = reply(&base, "a", p, "one more", false).await["seq"].as_i64().unwrap();
    call(&base, "b", "POST", &format!("{ROOM}/threads/{p}/read"), Some(json!({ "seq": r3 }))).await;
    let told = || mine.lock().unwrap().matches("\"type\":\"threadread\"").count();
    for _ in 0..100 {
        if told() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(told(), 2, "{}", mine.lock().unwrap());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!other.lock().unwrap().contains("threadread"));
}

/// Deleting the parent blanks it and keeps its thread: replies still read
/// and take replies; a deleted message with no thread takes none.
#[tokio::test]
async fn deleting_the_parent_keeps_its_thread() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let p = say(&base, "a", "oops").await;
    reply(&base, "b", p, "a reply", false).await;
    let (st, _) = call(&base, "a", "DELETE", &format!("{ROOM}/messages/{p}"), None).await;
    assert_eq!(st, 200);
    let main = list(&base, "b").await;
    assert_eq!(
        (&main[0]["text"], main[0]["deleted"].is_u64(), &main[0]["thread"]["replies"]),
        (&json!(""), true, &json!(1)),
        "{main:?}"
    );
    let v = thread(&base, "b", p).await;
    assert_eq!((v["messages"].as_array().unwrap().len(), v["parent"]["deleted"].is_u64()), (1, true));
    reply(&base, "b", p, "still here", false).await;
    assert_eq!(thread(&base, "a", p).await["messages"].as_array().unwrap().len(), 2);

    let q = say(&base, "a", "gone before anyone answered").await;
    call(&base, "a", "DELETE", &format!("{ROOM}/messages/{q}"), None).await;
    let (st, _) =
        call(&base, "b", "POST", &format!("{ROOM}/messages"), Some(json!({ "text": "late", "parent": q }))).await;
    assert_eq!(st, 409);

    // A deleted reply leaves the count.
    let r = reply(&base, "b", p, "oops too", false).await["seq"].as_i64().unwrap();
    assert_eq!(list(&base, "a").await[0]["thread"]["replies"], 3);
    call(&base, "b", "DELETE", &format!("{ROOM}/messages/{r}"), None).await;
    assert_eq!(list(&base, "a").await[0]["thread"]["replies"], 2);
}

/// An agent replies in a thread it was asked in and reads it, as its
/// owner's agent; the thread's followers are told as for anyone's reply,
/// and its owner isn't made to follow or read it by what it does.
#[tokio::test]
async fn an_agent_replies_in_a_thread_and_reads_it() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let p = say(&base, "b", "@geek can you check the logs?").await;
    let scope = json!({ "kind": "project", "id": t });
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;
    let post = "/api/daemon/chat/rooms/ch_general/messages".to_string();
    let body = Some(json!({ "text": "on it", "parent": p }));
    let geek = |method: &'static str, path: String, body: Option<Value>| {
        let base = base.clone();
        async move {
            let mut req = reqwest::Client::new()
                .request(method.parse().unwrap(), format!("{base}{path}"))
                .header("x-test-daemon", "a geek");
            if let Some(b) = body {
                req = req.json(&b);
            }
            let r = req.send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default())
        }
    };
    let (st, _) = geek("POST", post.clone(), body.clone()).await;
    assert_eq!(st, 403, "reading isn't posting");
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, scope)).await;
    let (st, v) = geek("POST", post.clone(), body).await;
    assert_eq!(st, 200, "{v}");
    let r = v["seq"].as_i64().unwrap();

    // Not in the room's list; in the thread, as the agent's; the parent says so.
    let (_, main) = geek("GET", post.clone(), None).await;
    // (The grant's own system lines, #122, are in the room too.)
    let said: Vec<&Value> = main["messages"].as_array().unwrap().iter().filter(|m| m["via"] != "system").collect();
    assert_eq!(said.len(), 1, "{main}");
    assert_eq!(said[0]["thread"]["replies"], 1);
    assert_eq!(said[0]["thread"]["following"], false, "an agent follows nothing");
    let (st, th) = geek("GET", format!("{post}?thread={p}"), None).await;
    assert_eq!(st, 200, "{th}");
    assert_eq!(
        (&th["messages"][0]["seq"], &th["messages"][0]["parent"], &th["parent"]["seq"]),
        (&json!(r), &json!(p), &json!(p))
    );
    assert_eq!(th["messages"][0]["agent"]["machine"], "geek");
    // The person who asked is a follower, with it unread; a's own isn't.
    assert_eq!(list(&base, "b").await[0]["thread"]["unread"], 1);
    assert_eq!(list(&base, "a").await[0]["thread"]["following"], false, "the owner didn't reply");
}

/// `@arugula` (M256) from a viewer, or from an editor who isn't on the
/// hosted beta's list, starts nothing and says why in the thread; in a DM
/// it says where it works. The word names nobody.
#[tokio::test]
async fn arugula_from_a_viewer_or_an_unlisted_member_starts_nothing() {
    use crate::sessions_hydrate_tests::{lines, platform, post_in, sessions};
    let (_hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    let p = post_in(&base, "ch_general", "x", "@arugula fix the canary", None).await;
    assert_eq!(p["unreached"], json!([]), "{p}");
    let p = p["seq"].as_i64().unwrap();
    assert_eq!(
        lines(&base, "ch_general", Some(p)).await,
        ["@arugula needs an owner or editor of this project to ask: x can only view it."]
    );
    let q = post_in(&base, "ch_general", "y", "@arugula fix the canary", None).await["seq"].as_i64().unwrap();
    assert_eq!(
        lines(&base, "ch_general", Some(q)).await,
        ["@arugula runs on cloud workspaces, which are in a limited beta: y isn't on its list yet."]
    );
    // A reply from someone who may doesn't pick up another's refused ask.
    post_in(&base, "ch_general", "b", "dev", Some(p)).await;
    assert!(sessions(&app, &t).is_empty());
    // Nowhere but a project's threads.
    let dm = crate::db::chat::Room {
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
    let d = post_in(&base, "dm_ab", "a", "@arugula hi", None).await["seq"].as_i64().unwrap();
    assert_eq!(lines(&base, "dm_ab", Some(d)).await, [crate::chat::sessions::NOT_HERE], "in the message's thread");
    assert!(sessions(&app, &t).is_empty());
}

/// A reply in a session's thread is a prompt, credited to its author, from
/// the asker or any owner or editor; queued while a turn runs, and the
/// session's job looks again at once. A viewer's reply is only chat.
#[tokio::test]
async fn a_reply_mid_turn_is_a_queued_prompt() {
    use crate::{
        db::sessions::{PromptState, State},
        sessions_hydrate_tests::{platform, post_in, sessions, take},
    };
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    let p = post_in(&base, "ch_general", "b", "@arugula fix the canary", None).await["seq"].as_i64().unwrap();
    let s = sessions(&app, &t).pop().unwrap();
    take(&app, &keys, &s.id);
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().state, State::Working);
    post_in(&base, "ch_general", "a", "also check the logs", Some(p)).await;
    post_in(&base, "ch_general", "x", "+1", Some(p)).await;
    post_in(&base, "ch_general", "b", "and the flaky test", Some(p)).await;
    // In the channel, not the thread: nothing to do with it.
    post_in(&base, "ch_general", "a", "lunch?", None).await;
    let prompts: Vec<(String, String, PromptState)> = app
        .db
        .session_prompts(&s.id)
        .unwrap()
        .into_iter()
        .map(|p| (p.author, if p.text.len() > 40 { "<first>".into() } else { p.text }, p.state))
        .collect();
    assert_eq!(
        prompts,
        [
            ("b".into(), "<first>".into(), PromptState::Sent),
            ("a".into(), "also check the logs".into(), PromptState::Queued),
            ("b".into(), "and the flaky test".into(), PromptState::Queued),
        ]
    );
    let job = app.db.platform_job(&s.id).unwrap().unwrap();
    assert!(job.next_at <= arugula_e2e::now_ms(), "nudged: {job:?}");
}

/// Several repos on the box: an ask (Arugula #214) asks which, and the
/// asker's reply naming one starts the session there; anyone else's, or
/// one naming none, is chat. Several boxes and none chosen: the pool picks
/// (`sessions_pool_tests`), and a reply naming a box still runs there. No
/// box: a line says to set one up. The session's first prompt leaves out
/// the question and its answer.
#[tokio::test]
async fn arugula_asks_which_repo() {
    use crate::sessions_hydrate_tests::{add_box, lines, platform, post_in, sessions};
    let (_hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app", "https://github.com/o/web.git"]).await;
    let p = post_in(&base, "ch_general", "b", "@arugula fix the canary", None).await["seq"].as_i64().unwrap();
    let ask = "Which repo should @arugula work in on dev? Reply with one: o/app or o/web.";
    let asks = |th: Value| -> Vec<String> {
        let ms = th["messages"].as_array().unwrap().clone();
        ms.iter().filter(|m| m["via"] == "ask:repo").map(|m| m["text"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(asks(thread(&base, "a", p).await), [ask]);
    post_in(&base, "ch_general", "a", "web", Some(p)).await;
    post_in(&base, "ch_general", "b", "hmm, not sure", Some(p)).await;
    assert!(sessions(&app, &t).is_empty());
    post_in(&base, "ch_general", "b", "web, please", Some(p)).await;
    let s = sessions(&app, &t).pop().expect("started");
    assert_eq!((s.repo.as_deref(), s.workspace.as_str(), s.parent_seq), (Some("o/web"), "hw_dev", p as u64));
    assert_eq!(s.branch, format!("arugula/{}-fix-the-canary", &s.id[4..12]));
    // The session's card (M257) says it started.
    let said = crate::sessions_hydrate_tests::cards(&base, "ch_general", Some(p)).await;
    assert_eq!(said, [format!("session:{}", s.id)]);
    // The question and the answer were for the platform: the first prompt
    // leaves them out, and keeps the rest (a's "web" is someone else's).
    let first = app.db.session_prompts(&s.id).unwrap().remove(0).text;
    assert!(
        first.contains("b (editor): @arugula fix the canary\na (owner): web\nb (editor): hmm, not sure\n\nRules:"),
        "{first}"
    );
    assert!(!first.contains("web, please") && !first.contains("Which repo"), "{first}");

    // A second box, none chosen: the ask names app, which both have; the
    // reply naming staging runs there.
    add_box(&app, &t, "hw_stage", "staging", &["https://github.com/o/app"], None);
    let q = post_in(&base, "ch_general", "b", "@arugula deploy it", None).await["seq"].as_i64().unwrap();
    assert_eq!(
        asks(thread(&base, "a", q).await),
        ["Which repo should @arugula work in? Reply with one: o/app or o/web."]
    );
    post_in(&base, "ch_general", "b", "staging", Some(q)).await;
    let s = sessions(&app, &t).into_iter().find(|s| s.parent_seq == q as u64).expect("started on staging");
    assert_eq!((s.workspace.as_str(), s.repo.as_deref()), ("hw_stage", Some("o/app")));
    let first = app.db.session_prompts(&s.id).unwrap().remove(0).text;
    assert!(first.contains("b (editor): @arugula deploy it\n\nRules:"), "{first}");
    assert!(!first.contains("Which cloud workspace"), "{first}");

    // No box at all.
    crate::db::sql::Sql::execute(&app.db.c(), "UPDATE hosted_workspaces SET deleted_at = 1", crate::db::sql::params![])
        .unwrap();
    let r = post_in(&base, "ch_general", "b", "@arugula again", None).await["seq"].as_i64().unwrap();
    assert_eq!(lines(&base, "ch_general", Some(r)).await, [crate::chat::sessions::NO_BOX]);
}
