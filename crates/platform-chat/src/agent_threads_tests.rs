//! MCP's thread routes (M198): a machine's agents list, read, search and
//! post in what its owner's grants reach, signed as the machine; mentions
//! of an agent are kept and read back, and push nobody.

use arugula_control_wire::{
    chat::{AGENT_ROOMS, AGENT_SEARCH, agent_messages},
    forge::{DaemonOp, OpAnswer},
};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::{
        issues::{Actor, NewIssue},
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
    },
    grants::AGENT_UNGRANTED,
};

/// A call signed by a's machine geek, as the hub's gate vouches for it.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

fn say(text: &str) -> Option<Value> {
    Some(json!({ "text": text }))
}

fn ids(v: &Value) -> Vec<&str> {
    v["rooms"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect()
}

/// Nothing without a grant (refused saying how); then the rooms a's grants
/// reach and nothing else, read and posted in as "a's agent on geek-box";
/// searched only there; and the next call after a revoke, or after a
/// leaves the team, reaches less.
#[tokio::test]
async fn an_agent_lists_reads_searches_and_posts_where_granted() {
    let (hub, app, base, t, _u) = seeded().await;
    let general = agent_messages("ch_general");

    let (st, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert_eq!((st, &v), (200, &json!({ "rooms": [], "reach": [] })));
    let (st, v) = geek(&base, "GET", &general, None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("arugula grants add geek-box threads.read"), "{v}");
    let (st, _) = geek(&base, "POST", &general, say("hello")).await;
    assert_eq!(st, 403);
    let (st, v) = geek(&base, "GET", &format!("{AGENT_SEARCH}?q=hello"), None).await;
    assert_eq!((st, &v["found"]), (200, &json!([])));

    // a gives geek t's threads, and reading (not posting) in a's DMs.
    let mut read_t = Value::Null;
    for (power, scope) in [
        (THREADS_READ, json!({ "kind": "project", "id": t })),
        (THREADS_POST, json!({ "kind": "project", "id": t })),
        (THREADS_READ, json!({ "kind": "account" })),
    ] {
        let (st, g) = call(&base, "a", "POST", "/api/grants", give("geek", power, scope)).await;
        assert_eq!(st, 200, "{g}");
        if power == THREADS_READ && g["scope"]["kind"] == "project" {
            read_t = g;
        }
    }
    let (st, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ids(&v), ["ch_general", "dm_ab"], "not b and x's DM: {v}");
    let rooms = v["rooms"].as_array().unwrap();
    assert_eq!((&rooms[0]["powers"], &rooms[0]["writable"]), (&json!(["threads.post", "threads.read"]), &json!(true)));
    assert_eq!((&rooms[1]["powers"], &rooms[1]["writable"]), (&json!(["threads.read"]), &json!(false)));
    assert_eq!(rooms[1]["with"]["account"], "b");
    assert_eq!(
        v["reach"],
        json!([{ "kind": "account", "id": "a", "name": "your DMs", "powers": ["threads.read"] },
               { "kind": "project", "id": t, "name": "t", "powers": ["threads.post", "threads.read"] }])
    );

    // b asks geek's agent by name; geek's daemon is connected and takes it, so
    // the word reached someone, and no device is pushed.
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (st, asked) =
        call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@geek-box's agent: look")).await;
    assert_eq!(st, 200, "{asked}");
    assert_eq!((&asked["mentions"], &asked["unreached"]), (&json!(["machine:geek"]), &json!([])));
    // The machine is told after the post returns (#419): its status line says how it went.
    assert_eq!(asked["pushed"], json!([]));
    assert_eq!(waits_for_told(&hub, 1).await.len(), 1);
    assert!(hub.pushes().is_empty(), "nobody pushed: {:?}", hub.pushes());
    call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("and something else")).await;

    // The agent reads the room, and only what mentions it.
    let (st, v) = geek(&base, "GET", &general, None).await;
    assert_eq!((st, v["messages"].as_array().map(Vec::len)), (200, Some(4)), "{v}");
    // Two lines first: the grants said so in the room (#122).
    assert_eq!((&v["room"]["last"], &v["room"]["last_mention"]), (&json!(4), &json!(3)));
    let (_, v) = geek(&base, "GET", &format!("{general}?mentions=true"), None).await;
    let texts: Vec<&Value> = v["messages"].as_array().unwrap().iter().map(|m| &m["text"]).collect();
    assert_eq!(texts, [&json!("@geek-box's agent: look")]);
    let (_, v) = geek(&base, "GET", &format!("{general}?after=3"), None).await;
    assert_eq!(v["messages"][0]["seq"], 4, "paged by seq: {v}");
    let (_, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert_eq!(v["rooms"][0]["last_mention"], 3);

    // It answers, labelled as a's agent; a DM it only reads it can't post in.
    let (st, posted) = geek(&base, "POST", &general, say("on it, @b")).await;
    assert_eq!(st, 200, "{posted}");
    assert_eq!(posted["mentions"], json!(["b"]));
    let (_, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general/messages", None).await;
    let m = &v["messages"][4];
    assert_eq!((&m["author"], &m["text"]), (&json!("a"), &json!("on it, @b")));
    assert_eq!(m["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }));
    let (st, v) = geek(&base, "POST", &agent_messages("dm_ab"), say("hi")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (st, _) = geek(&base, "GET", &agent_messages("dm_bx"), None).await;
    assert_eq!(st, 404, "b and x's DM isn't there for a");

    // Search: only the rooms it reads.
    call(&base, "b", "POST", "/api/chat/rooms/dm_bx/messages", say("look at this secret")).await;
    call(&base, "b", "POST", "/api/chat/rooms/dm_ab/messages", say("look here too")).await;
    let (st, v) = geek(&base, "GET", &format!("{AGENT_SEARCH}?q=look"), None).await;
    assert_eq!(st, 200, "{v}");
    let mut rooms: Vec<&str> = v["found"].as_array().unwrap().iter().map(|h| h["room"].as_str().unwrap()).collect();
    rooms.sort();
    assert_eq!(rooms, ["ch_general", "dm_ab"], "{v}");

    // Revoked: the next call reads no more of t's channel; posting stays.
    let id = read_t["id"].as_str().unwrap();
    assert_eq!(call(&base, "a", "POST", &format!("/api/grants/{id}/revoke"), None).await.0, 200);
    let (st, v) = geek(&base, "GET", &general, None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (_, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert_eq!(v["rooms"][0]["powers"], json!(["threads.post"]), "{v}");
    let (_, v) = geek(&base, "GET", &format!("{AGENT_SEARCH}?q=look"), None).await;
    assert_eq!(v["found"].as_array().unwrap().len(), 1, "the DM only: {v}");
    let (_, v) = geek(&base, "GET", &format!("{AGENT_SEARCH}?q=look&team=t"), None).await;
    assert_eq!(v["found"][0]["room"], "dm_ab");

    // a leaves t (with chat on for a still): nothing of t's is reached,
    // a's DMs there included, though the grants are live.
    hub.flag("account:a", true);
    hub.team("t", &[("b", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ids(&v), Vec::<&str>::new(), "{v}");
    assert_eq!(v["reach"], json!([{ "kind": "account", "id": "a", "name": "your DMs", "powers": ["threads.read"] }]));
    let (st, _) = geek(&base, "POST", &general, say("still here?")).await;
    assert_eq!(st, 404, "a doesn't see it any more");
    let (st, v) = geek(&base, "GET", &format!("{AGENT_SEARCH}?q=look"), None).await;
    assert_eq!((st, &v["found"]), (200, &json!([])));
}

/// `@<machine>` is a mention only where a grant of its reaches the room:
/// elsewhere the word reaches nobody. A person's mention beside it is
/// pushed as ever; an edit works mentions out again.
#[tokio::test]
async fn an_agent_is_mentioned_only_where_it_reaches() {
    let (hub, _app, base, t, _u) = seeded().await;
    let (st, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@geek-box hi")).await;
    assert_eq!((st, &v["mentions"], &v["unreached"]), (200, &json!([]), &json!(["geek-box"])), "no grant yet");

    // Posting alone isn't enough: it couldn't read the mention. (Each grant
    // says so in the room, a line of its own: that is why the numbers below skip.)
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, json!({ "kind": "project", "id": t }))).await;
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@geek-box hi")).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!([]), &json!(["geek-box"])), "post only: {v}");
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t }))).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@a and @GEEK-box")).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["a", "machine:geek"]), &json!([])), "{v}");
    let pushed: Vec<&str> = v["pushed"].as_array().unwrap().iter().map(|p| p["account"].as_str().unwrap()).collect();
    assert_eq!(pushed, ["a"], "the person is pushed; the machine is told in the background (#419)");
    assert!(hub.pushes().iter().all(|(to, _, _)| to == "a"), "{:?}", hub.pushes());
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/dm_bx/messages", say("@geek-box?")).await;
    assert_eq!(v["unreached"], json!(["geek-box"]), "a's grant doesn't reach b and x's DM");

    let (st, v) = call(&base, "b", "PATCH", "/api/chat/rooms/ch_general/messages/5", say("@a only")).await;
    assert_eq!((st, &v["mentions"]), (200, &json!(["a"])), "{v}");
    let (_, v) = geek(&base, "GET", &format!("{}?mentions=true", agent_messages("ch_general")), None).await;
    assert_eq!(v["messages"], json!([]), "{v}");
    let (_, v) = call(&base, "b", "PATCH", "/api/chat/rooms/ch_general/messages/1", say("@geek-box hi")).await;
    assert_eq!(v["mentions"], json!(["machine:geek"]));
    let (_, v) = geek(&base, "GET", &format!("{}?mentions=true", agent_messages("ch_general")), None).await;
    assert_eq!(v["messages"][0]["seq"], 1, "{v}");
}

/// An agent reads and comments on a native issue by its id: the thread is
/// made the first time, listed from then on, and the comment labelled.
#[tokio::test]
async fn an_agent_comments_on_an_issue_by_its_id() {
    let (_hub, app, base, t, u) = seeded().await;
    let issue = |project: &str| {
        let n = NewIssue { project, title: "the canary fails", body: "", labels: &[], assignees: &[] };
        app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap()
    };
    let ours = issue(&t);
    let theirs = issue(&u);
    let (st, v) = geek(&base, "POST", &agent_messages(&ours.id), say("bisected it")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    // issues.write alone doesn't read the thread: no mention of geek there.
    call(&base, "a", "POST", "/api/grants", give("geek", ISSUES_WRITE, json!({ "kind": "project", "id": t }))).await;
    let (st, opened) =
        call(&base, "b", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": ours.id }))).await;
    assert_eq!(st, 200, "{opened}");
    let detail = call(&base, "b", "GET", &format!("/api/chat/rooms/{}", opened["id"].as_str().unwrap()), None).await.1;
    assert_eq!(detail["agents"][0]["powers"], json!(["issues.write"]), "disclosed, all the same: {detail}");
    let path = format!("/api/chat/rooms/{}/messages", opened["id"].as_str().unwrap());
    let (_, v) = call(&base, "b", "POST", &path, say("@geek-box can you fix it?")).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!([]), &json!(["geek-box"])), "{v}");
    for power in [THREADS_READ, THREADS_POST] {
        call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
    }
    let (st, v) = geek(&base, "POST", &agent_messages(&ours.id), say("bisected it")).await;
    assert_eq!((st, &v["seq"]), (200, &json!(2)), "{v}");
    let (st, v) = geek(&base, "GET", &agent_messages(&ours.id), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((&v["room"]["kind"], &v["room"]["target"]), (&json!("object"), &json!(ours.id)));
    assert_eq!(v["room"]["project"], json!(t));
    assert_eq!(v["messages"][1]["agent"]["machine_name"], "geek-box");
    let room = v["room"]["id"].as_str().unwrap().to_owned();
    let (_, v) = geek(&base, "GET", AGENT_ROOMS, None).await;
    assert!(ids(&v).contains(&room.as_str()), "{v}");
    let (st, _) = geek(&base, "GET", &agent_messages(&theirs.id), None).await;
    assert_eq!(st, 404, "u's issue isn't a's to see");
}

/// Gives geek (a's machine) threads in t, as `an_agent_is_mentioned_only_where_it_reaches` does.
async fn geek_in_t(base: &str, t: &str) {
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
}

/// The ops asked of geek, as `(room, seq, parent, author, text)`.
fn told(hub: &crate::testing::hub::TestHub) -> Vec<(String, i64, Option<i64>, String, String)> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentMention(a) if m == "geek" => Some((a.room, a.seq, a.parent, a.author, a.text)),
            _ => None,
        })
        .collect()
}

/// The ops asked of geek, once there are `n` (they go after the post returns).
async fn waits_for_told(
    hub: &crate::testing::hub::TestHub,
    n: usize,
) -> Vec<(String, i64, Option<i64>, String, String)> {
    for _ in 0..100 {
        if told(hub).len() >= n {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    told(hub)
}

/// The state of the first machine's line under message `seq` of the room, once it isn't `sending`.
async fn settled_state(base: &str, who: &str, seq: i64) -> String {
    for _ in 0..100 {
        let (_, v) = call(base, who, "GET", "/api/chat/rooms/ch_general/messages", None).await;
        let state = v["messages"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m["seq"] == seq))
            .and_then(|m| m["mention_status"][0]["state"].as_str())
            .map(str::to_owned);
        if let Some(s) = state.filter(|s| s != "sending") {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("message {seq}'s mention never left `sending`")
}

/// `@geek-box` with geek connected is told to its daemon after the post
/// returns; with it offline, or its owner's switch off, the mention is
/// still stored for the agents to read, and the message's status line says
/// what happened (#419: no longer `pushed` or `unreached`).
#[tokio::test]
async fn a_mention_is_told_to_the_machine_or_says_it_was_not() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let post = |text: &'static str| call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say(text));

    let state_of = |seq: i64| {
        let base = base.clone();
        async move { settled_state(&base, "b", seq).await }
    };

    // Offline (the test hub's default): not told, and the line says so.
    let (_, v) = post("@geek-box are you there").await;
    assert_eq!(
        (&v["mentions"], &v["unreached"], &v["pushed"]),
        (&json!(["machine:geek"]), &json!([]), &json!([])),
        "{v}"
    );
    assert_eq!(state_of(v["seq"].as_i64().unwrap()).await, "offline");

    // The owner turned mentions off: the daemon says so.
    hub.answers_ops("geek", OpAnswer::Cant { why: "off".into() });
    let (_, v) = post("@geek-box now?").await;
    assert_eq!((&v["unreached"], &v["pushed"]), (&json!([]), &json!([])), "{v}");
    assert_eq!(state_of(v["seq"].as_i64().unwrap()).await, "offline");

    // Connected: told, and delivered.
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (_, v) = post("@geek-box what is the board for").await;
    assert_eq!((&v["unreached"], &v["pushed"]), (&json!([]), &json!([])));
    assert_eq!(state_of(v["seq"].as_i64().unwrap()).await, "delivered");
    let sent = waits_for_told(&hub, 1).await;
    let last = sent.last().unwrap();
    assert_eq!((last.0.as_str(), last.2, last.4.as_str()), ("ch_general", None, "@geek-box what is the board for"));
    assert_eq!(last.3, "b", "{sent:?}");
}

/// The sessions the ops asked of geek named, in order (#429).
fn told_sessions(hub: &crate::testing::hub::TestHub) -> Vec<Option<String>> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentMention(a) if m == "geek" => Some(a.session),
            _ => None,
        })
        .collect()
}

/// An agent's post carries its session's name, shown beside the agent
/// (#429); a person's post can't carry one, and a blank name is none.
/// `@geek-box/ux-improvements` tells the daemon which session, and a plain
/// `@geek-box` doesn't.
#[tokio::test]
async fn a_post_names_its_session_and_a_mention_can_address_one() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let room = "/api/chat/rooms/ch_general/messages";
    let path = agent_messages("ch_general");
    let agent =
        |text: &str, session: Value| geek(&base, "POST", &path, Some(json!({ "text": text, "session": session })));

    let (st, v) = agent("from ux", json!("  UX\n Improvements ")).await;
    assert_eq!(st, 200, "{v}");
    agent("from nobody", json!("   ")).await;
    agent("from an old daemon", Value::Null).await;
    let (_, v) = call(&base, "b", "POST", room, Some(json!({ "text": "b is sneaky", "session": "Sneaky" }))).await;
    assert!(v["seq"].is_number(), "{v}");

    let (_, v) = geek(&base, "GET", &path, None).await;
    let shown: Vec<(String, Value)> = v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .skip(2) // the room's two notices that geek can post and read
        .map(|m| (m["text"].as_str().unwrap().to_owned(), m["agent"]["session"].clone()))
        .collect();
    assert_eq!(
        shown,
        [
            ("from ux".to_owned(), json!("UX Improvements")),
            ("from nobody".to_owned(), Value::Null),
            ("from an old daemon".to_owned(), Value::Null),
            ("b is sneaky".to_owned(), Value::Null),
        ]
    );
    let (_, v) = call(&base, "b", "GET", room, None).await;
    let last = v["messages"].as_array().unwrap().iter().find(|m| m["text"] == "b is sneaky").unwrap().clone();
    assert!(last["agent"].is_null(), "a person's post has no agent, so no session: {last}");

    for text in ["@geek-box/UX-Improvements look at this", "@geek-box and this", "@geek-box/ look", "@GEEK-BOX/billing"]
    {
        let (_, v) = call(&base, "b", "POST", room, say(text)).await;
        assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["machine:geek"]), &json!([])), "{text}: {v}");
    }
    for _ in 0..100 {
        if told_sessions(&hub).len() >= 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(told_sessions(&hub), [Some("ux-improvements".to_owned()), None, None, Some("billing".to_owned())]);
}

/// A reply under a message of an agent's is for that agent's machine, with
/// no mention; a reply under a person's, or the agent's own, is for nobody.
#[tokio::test]
async fn a_reply_to_an_agents_message_is_told_to_its_machine() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let room = "/api/chat/rooms/ch_general/messages";

    // b starts a thread; a reply under b's own message tells no one.
    let (_, root) = call(&base, "b", "POST", room, say("a question for the board")).await;
    let root = root["seq"].as_i64().unwrap();
    let reply = |text: &'static str| call(&base, "b", "POST", room, Some(json!({ "text": text, "parent": root })));
    let (_, v) = reply("more on it").await;
    assert_eq!(v["pushed"], json!([]));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(told(&hub).is_empty());

    // geek's agent answers in the thread; b's next reply there is for it.
    let (st, v) =
        geek(&base, "POST", &agent_messages("ch_general"), Some(json!({ "text": "my view", "parent": root }))).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = reply("thanks, and one more thing").await;
    assert_eq!(v["pushed"], json!([]), "{v}");
    let sent = waits_for_told(&hub, 1).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0].2, sent[0].4.as_str()), (Some(root), "thanks, and one more thing"));

    // The agent's own reply isn't told to itself, and a person's reply
    // after b's isn't an answer to the agent's message.
    let (_, v) =
        geek(&base, "POST", &agent_messages("ch_general"), Some(json!({ "text": "ok", "parent": root }))).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(told(&hub).len(), 1, "{v}");
    let (_, v) = reply("and then b says more").await;
    assert_eq!(v["pushed"], json!([]), "{v}");
    assert_eq!(waits_for_told(&hub, 2).await.len(), 2, "the agent spoke last, so it is told");
    let (_, v) = call(&base, "a", "POST", room, Some(json!({ "text": "a chimes in", "parent": root }))).await;
    assert_eq!(v["pushed"].as_array().map(|p| p.iter().filter(|p| p["why"] == "agent").count()), Some(0), "{v}");
}

/// The machines `@` offers in a room (#428): the ones a mention reaches
/// (a live `threads.read` grant over the room), each with the sessions its
/// agents posted from there this week, newest first, five at most. A
/// machine with only `threads.post`, or with no grant, isn't offered, and
/// neither are sessions that posted elsewhere, long ago, or were deleted.
#[tokio::test]
async fn the_room_offers_the_machines_a_mention_reaches_and_their_sessions() {
    use crate::db::chat::NewMessage;
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.daemon("mini", "a", Some("t"));
    hub.daemon("pad", "a", Some("t"));
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("mini", THREADS_POST, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let path = agent_messages("ch_general");
    for n in 1..=7 {
        let (st, v) =
            geek(&base, "POST", &path, Some(json!({ "text": "hi", "session": format!("Session {n}") }))).await;
        assert_eq!(st, 200, "{v}");
    }
    // Session 1 speaks again: the newest, once.
    geek(&base, "POST", &path, Some(json!({ "text": "again", "session": "Session 1" }))).await;
    let now = arugula_e2e::now_ms();
    let post = |room: &str, at: u64, session: &str| {
        let m = NewMessage {
            author: "a",
            at,
            text: "old",
            quote: None,
            ref_machine: None,
            ref_pane: None,
            mentions: &[],
            agent: Some("geek"),
            via: None,
            parent: None,
            also: false,
            agent_session: Some(session),
        };
        app.db.add_chat_message(room, &m).unwrap()
    };
    post("ch_general", now - 8 * 24 * 60 * 60 * 1000, "Last Month");
    post("dm_ab", now, "Elsewhere");
    let gone = post("ch_general", now, "Deleted");
    app.db.delete_chat_message("ch_general", gone, now).unwrap();

    let (_, v) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    let offered: Vec<(&str, bool, Vec<&str>)> = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let s =
                a["sessions"].as_array().map(|s| s.iter().map(|s| s.as_str().unwrap()).collect()).unwrap_or_default();
            (a["machine"].as_str().unwrap(), a["mentionable"].as_bool().unwrap(), s)
        })
        .collect();
    assert_eq!(
        offered,
        [
            ("geek", true, vec!["Session 1", "Session 7", "Session 6", "Session 5", "Session 4"]),
            ("mini", false, vec![]),
        ],
        "pad has no grant, mini can only post: {v}"
    );

    // What is offered is what a mention reaches: geek is pushed, mini isn't.
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    hub.answers_ops("mini", OpAnswer::Done { url: None, routed: None });
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@geek-box and @mini-box")).await;
    assert_eq!(v["pushed"], json!([]), "{v}");
    assert_eq!(v["unreached"], json!(["mini-box"]), "{v}");
    assert_eq!(waits_for_told(&hub, 1).await.len(), 1);
}

/// A word that names a machine of the team but reaches nobody says why
/// (#572): its agents can't read the room, and the rooms they do read: only
/// those the poster sees too. A private room the poster isn't in stays unnamed.
#[tokio::test]
async fn an_unreached_machine_says_which_rooms_it_reads() {
    let (hub, _app, base, t, _u) = seeded().await;
    let channels = format!("/api/chat/teams/{}/channels", "t");
    let make = |who: &'static str, name: &'static str, private: bool| {
        let base = base.clone();
        let channels = channels.clone();
        async move {
            let (st, v) = call(&base, who, "POST", &channels, Some(json!({ "name": name, "private": private }))).await;
            assert_eq!(st, 200, "{v}");
            v["id"].as_str().unwrap().to_owned()
        }
    };
    // geek's owner (a) is in #vault and not in #bs: b's own private room.
    let _vault = make("a", "vault", true).await;
    let bs = make("b", "bs", true).await;
    let post = |room: String| {
        let base = base.clone();
        async move { call(&base, "b", "POST", &format!("/api/chat/rooms/{room}/messages"), say("@geek-box hi")).await.1 }
    };

    // No grant: reached nobody, and it reads no room.
    let v = post("ch_general".into()).await;
    assert_eq!(v["unreached"], json!(["geek-box"]), "{v}");
    assert_eq!(v["unreached_why"], json!([{ "word": "geek-box", "machine_name": "geek-box", "reads": [] }]), "{v}");

    // Granted over the team: #bs is b's, geek's owner isn't in it. geek reads
    // the others, but b isn't in #vault: its name isn't b's to see.
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t }))).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let v = post(bs.clone()).await;
    assert_eq!(v["unreached"], json!(["geek-box"]), "{v}");
    assert_eq!(v["unreached_why"][0]["reads"], json!(["#ch_general"]), "{v}");
    assert!(!v.to_string().contains("vault"), "a private room b isn't in: {v}");

    // Where it reads, nothing changes: reached, no reason.
    let v = post("ch_general".into()).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["machine:geek"]), &json!([])), "{v}");
    assert!(v["unreached_why"].is_null() || v["unreached_why"] == json!([]), "{v}");

    // A word that names no machine has no reason.
    let (_, v) = call(&base, "b", "POST", &format!("/api/chat/rooms/{bs}/messages"), say("@nobody")).await;
    assert_eq!(v["unreached"], json!(["nobody"]), "{v}");
    assert!(v["unreached_why"].is_null() || v["unreached_why"] == json!([]), "{v}");
}

/// The mentions asked of `machine`, whole.
fn asked_of(hub: &crate::testing::hub::TestHub, machine: &str) -> Vec<arugula_control_wire::forge::AgentMention> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentMention(a) if m == machine => Some(a),
            _ => None,
        })
        .collect()
}

/// `machine`'s agent posts `text` as `session` under `parent` (a root if none); the message's number.
async fn agent_says(base: &str, machine: &str, session: &str, text: &str, parent: Option<i64>) -> i64 {
    let r = reqwest::Client::new()
        .post(format!("{base}{}", agent_messages("ch_general")))
        .header("x-test-daemon", format!("a {machine}"))
        .json(&json!({ "text": text, "session": session, "parent": parent }))
        .send()
        .await
        .unwrap();
    let v = r.json::<Value>().await.unwrap();
    v["seq"].as_i64().unwrap_or_else(|| panic!("{v}"))
}

/// The state of the first machine's line under reply `seq` in the thread of `root`, once it isn't `sending`.
async fn thread_state(base: &str, root: i64, seq: i64) -> String {
    for _ in 0..100 {
        let path = format!("/api/chat/rooms/ch_general/messages?thread={root}");
        let (_, v) = call(base, "b", "GET", &path, None).await;
        let state = v["messages"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m["seq"] == seq))
            .and_then(|m| m["mention_status"][0]["state"].as_str())
            .map(str::to_owned);
        if let Some(s) = state.filter(|s| s != "sending") {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("message {seq}'s mention never left `sending`")
}

/// An agent's mention of its own machine is told to it (#575), with the
/// session that wrote it so the daemon can keep it from that one: named,
/// bare, and a reply under another session's message. A reply under its own
/// session's message, or one with no sessions to tell apart, is for no one.
#[tokio::test]
async fn an_agents_mention_of_its_own_machine_is_told_with_its_author() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let sessions = |hub: &crate::testing::hub::TestHub| {
        asked_of(hub, "geek").into_iter().map(|a| (a.session, a.from_session)).collect::<Vec<_>>()
    };
    let some = |s: &str| Some(s.to_owned());

    // Named, from a post in the room.
    agent_says(&base, "geek", "Alpha", "@geek-box/beta pick this up", None).await;
    assert_eq!(waits_for_told(&hub, 1).await.len(), 1);
    assert_eq!(sessions(&hub), [(some("beta"), some("Alpha"))]);

    // Bare, in a thread Alpha posted in last: control tells the machine and
    // says who wrote it; the daemon keeps it from Alpha's pane.
    let root = agent_says(&base, "geek", "Alpha", "my plan", None).await;
    let seq = agent_says(&base, "geek", "Alpha", "@geek-box anyone?", Some(root)).await;
    assert_eq!(waits_for_told(&hub, 2).await.len(), 2);
    assert_eq!(thread_state(&base, root, seq).await, "delivered");
    assert_eq!(sessions(&hub)[1], (None, some("Alpha")));

    // A plain reply under Alpha's own message is for no one; Beta's is for
    // the machine (another session may want it), and so is Alpha's under
    // Beta's. Each in a thread of its own: they would be a chain in one.
    let own = agent_says(&base, "geek", "Alpha", "another plan", None).await;
    agent_says(&base, "geek", "Alpha", "carrying on", Some(own)).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(told(&hub).len(), 2, "Alpha's reply under Alpha's message");
    agent_says(&base, "geek", "Beta", "a thought", Some(own)).await;
    assert_eq!(waits_for_told(&hub, 3).await.len(), 3);
    let theirs = agent_says(&base, "geek", "Beta", "beta's plan", None).await;
    agent_says(&base, "geek", "Alpha", "thanks", Some(theirs)).await;
    assert_eq!(waits_for_told(&hub, 4).await.len(), 4);
    assert_eq!(sessions(&hub)[2..], [(None, some("Beta")), (None, some("Alpha"))]);

    // Without a session name on either, nothing says they differ.
    let unnamed = agent_says(&base, "geek", "", "no name", None).await;
    agent_says(&base, "geek", "", "still none", Some(unnamed)).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(told(&hub).len(), 4, "no one to tell apart");
}

/// A chain of agents on one machine stops (#575): A → B → A → B, each
/// mentioning the machine under a person's message, delivers three and
/// then says `chain`; the person's next message restarts it.
#[tokio::test]
async fn a_chain_of_agents_on_one_machine_stops_and_says_so() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let room = "/api/chat/rooms/ch_general/messages";
    let (_, v) = call(&base, "b", "POST", room, say("@geek-box start")).await;
    let root = v["seq"].as_i64().unwrap();
    assert_eq!(waits_for_told(&hub, 1).await.len(), 1);

    let mut states = vec![];
    for (n, session) in ["Alpha", "Beta", "Alpha", "Beta"].into_iter().enumerate() {
        let seq = agent_says(&base, "geek", session, &format!("@geek-box/x step {n}"), Some(root)).await;
        states.push(thread_state(&base, root, seq).await);
    }
    assert_eq!(states, ["delivered", "delivered", "delivered", "chain"]);
    assert_eq!(told(&hub).len(), 4, "start and three steps; the fourth wasn't told");

    // A person's message restarts it, and the agent's next is told again.
    let (_, v) = call(&base, "b", "POST", room, Some(json!({ "text": "go on", "parent": root }))).await;
    assert!(v["seq"].as_i64().is_some());
    assert_eq!(waits_for_told(&hub, 5).await.len(), 5, "b's reply is for the agent that spoke last");
    let seq = agent_says(&base, "geek", "Alpha", "@geek-box/beta again", Some(root)).await;
    assert_eq!(thread_state(&base, root, seq).await, "delivered");
}

/// Another machine's agents are told at most ten times in a thread in ten
/// minutes (#575), a backstop; the eleventh says `chain`. The count forgets
/// after the window.
#[tokio::test]
async fn agents_are_told_at_most_ten_times_per_thread_per_machine_in_a_window() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.daemon("mini", "a", Some("t"));
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(&base, "a", "POST", "/api/grants", give("mini", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("a thread")).await;
    let root = v["seq"].as_i64().unwrap();
    let mut last = String::new();
    for n in 0..11 {
        let seq = agent_says(&base, "mini", "Mini", &format!("@geek-box ping {n}"), Some(root)).await;
        last = thread_state(&base, root, seq).await;
        if n < 10 {
            assert_eq!(last, "delivered", "{n}");
        }
    }
    assert_eq!(last, "chain");
    assert_eq!(asked_of(&hub, "geek").len(), 10);

    let now = arugula_e2e::now_ms();
    assert!(!app.chat.agent_told("ch_general", root, "geek", now));
    assert!(app.chat.agent_told("ch_general", root, "geek", now + crate::chat::AGENT_TOLD_WINDOW_MS));
}

/// An agent lists a teammate machine's sessions in a room (#577): the
/// asker reads the room and the target hears it. A machine that can only
/// post, or isn't granted, is no such machine there; an offline one answers
/// an empty list and says so; the asker's own session is left out only for
/// its own machine.
#[tokio::test]
async fn an_agent_lists_the_sessions_of_a_machine_that_hears_the_room() {
    use arugula_control_wire::{
        chat::agent_machine_sessions,
        forge::{DaemonOp, MachineSession},
    };
    let (hub, _app, base, t, _u) = seeded().await;
    hub.daemon("mini", "a", Some("t"));
    hub.daemon("pad", "a", Some("t"));
    let path = |m: &str| agent_machine_sessions("ch_general", m);
    let sessions_asked = || -> Vec<(String, Option<String>)> {
        hub.ops_asked()
            .into_iter()
            .filter_map(|(m, op)| match op {
                DaemonOp::AgentSessions(a) => Some((m, a.from_session)),
                _ => None,
            })
            .collect()
    };

    // No grant for the asker: it doesn't read the room.
    let (st, v) = geek(&base, "GET", &path("mini-box"), None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");

    for (m, power) in [("geek", THREADS_READ), ("mini", THREADS_READ), ("pad", THREADS_POST)] {
        let (st, g) =
            call(&base, "a", "POST", "/api/grants", give(m, power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }

    // pad can only post, nobody is called nope: neither hears the room.
    for m in ["pad-box", "nope"] {
        let (st, v) = geek(&base, "GET", &path(m), None).await;
        assert_eq!(st, 404, "{m}: {v}");
    }
    assert!(sessions_asked().is_empty(), "a machine that doesn't hear is never asked");

    // Offline (no daemon answers): empty, and it says so.
    let (st, v) = geek(&base, "GET", &path("mini-box"), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["offline"].as_bool(), v["sessions"].clone()), (Some(true), json!([])), "{v}");

    // Online: the daemon's list, as it said it. Another machine's asker
    // gives its session no mention.
    let slow = MachineSession {
        name: "slowness".into(),
        tab: Some("Views".into()),
        active_ms_ago: Some(180_000),
        working_on: Some("#540".into()),
    };
    hub.answers_ops("mini", OpAnswer::Sessions { shared: true, sessions: vec![slow] });
    let (st, v) = geek(&base, "GET", &format!("{}?from_session=Billing", path("@Mini-Box")), None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["offline"], false, "{v}");
    assert_eq!(v["sessions"][0]["name"], "slowness", "{v}");
    assert_eq!(v["sessions"][0]["working_on"], "#540", "{v}");
    assert_eq!(*sessions_asked().last().unwrap(), ("mini".to_owned(), None), "from_session is for its own machine");

    // Asked about its own machine, the asker's session goes with it.
    hub.answers_ops("geek", OpAnswer::Sessions { shared: false, sessions: vec![] });
    let (st, v) = geek(&base, "GET", &format!("{}?from_session=Billing", path("geek-box")), None).await;
    assert_eq!((st, v["shared"].as_bool(), v["offline"].as_bool()), (200, Some(false), Some(false)), "{v}");
    assert_eq!(*sessions_asked().last().unwrap(), ("geek".to_owned(), Some("Billing".to_owned())));
}
