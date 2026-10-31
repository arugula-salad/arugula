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
    hub.answers_ops("geek", OpAnswer::Done { url: None });
    let (st, asked) =
        call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@geek-box's agent: look")).await;
    assert_eq!(st, 200, "{asked}");
    assert_eq!((&asked["mentions"], &asked["unreached"]), (&json!(["machine:geek"]), &json!([])));
    assert_eq!(asked["pushed"], json!([{ "account": "machine:geek", "why": "agent" }]));
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
    hub.answers_ops("geek", OpAnswer::Done { url: None });
    let (_, v) = call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say("@a and @GEEK-box")).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["a", "machine:geek"]), &json!([])), "{v}");
    let pushed: Vec<&str> = v["pushed"].as_array().unwrap().iter().map(|p| p["account"].as_str().unwrap()).collect();
    assert_eq!(pushed, ["a", "machine:geek"], "the person is pushed, and the machine told");
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

/// `@geek-box` with geek connected is told to its daemon, and reached
/// (`pushed`); with it offline, or its owner's switch off, the word is
/// `unreached` and the mention is still stored for the agents to read.
#[tokio::test]
async fn a_mention_is_told_to_the_machine_or_says_it_was_not() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let post = |text: &'static str| call(&base, "b", "POST", "/api/chat/rooms/ch_general/messages", say(text));

    // Offline (the test hub's default): not told, the word unreached.
    let (_, v) = post("@geek-box are you there").await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["machine:geek"]), &json!(["geek-box"])), "{v}");
    assert_eq!(v["pushed"], json!([]));

    // The owner turned mentions off: the daemon says so.
    hub.answers_ops("geek", OpAnswer::Cant { why: "off".into() });
    let (_, v) = post("@geek-box now?").await;
    assert_eq!((&v["unreached"], &v["pushed"]), (&json!(["geek-box"]), &json!([])), "{v}");

    // Connected: told, and pushed as the machine.
    hub.answers_ops("geek", OpAnswer::Done { url: None });
    let (_, v) = post("@geek-box what is the board for").await;
    assert_eq!((&v["unreached"], &v["pushed"]), (&json!([]), &json!([{ "account": "machine:geek", "why": "agent" }])));
    let sent = told(&hub);
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
    hub.answers_ops("geek", OpAnswer::Done { url: None });
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
    assert_eq!(told_sessions(&hub), [Some("ux-improvements".to_owned()), None, None, Some("billing".to_owned())]);
}

/// A reply under a message of an agent's is for that agent's machine, with
/// no mention; a reply under a person's, or the agent's own, is for nobody.
#[tokio::test]
async fn a_reply_to_an_agents_message_is_told_to_its_machine() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None });
    let room = "/api/chat/rooms/ch_general/messages";

    // b starts a thread; a reply under b's own message tells no one.
    let (_, root) = call(&base, "b", "POST", room, say("a question for the board")).await;
    let root = root["seq"].as_i64().unwrap();
    let reply = |text: &'static str| call(&base, "b", "POST", room, Some(json!({ "text": text, "parent": root })));
    let (_, v) = reply("more on it").await;
    assert_eq!(v["pushed"], json!([]));
    assert!(told(&hub).is_empty());

    // geek's agent answers in the thread; b's next reply there is for it.
    let (st, v) =
        geek(&base, "POST", &agent_messages("ch_general"), Some(json!({ "text": "my view", "parent": root }))).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = reply("thanks, and one more thing").await;
    assert_eq!(v["pushed"], json!([{ "account": "machine:geek", "why": "agent" }]), "{v}");
    let sent = told(&hub);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0].2, sent[0].4.as_str()), (Some(root), "thanks, and one more thing"));

    // The agent's own reply isn't told to itself, and a person's reply
    // after b's isn't an answer to the agent's message.
    let (_, v) =
        geek(&base, "POST", &agent_messages("ch_general"), Some(json!({ "text": "ok", "parent": root }))).await;
    assert_eq!(told(&hub).len(), 1, "{v}");
    let (_, v) = reply("and then b says more").await;
    assert_eq!(v["pushed"], json!([{ "account": "machine:geek", "why": "agent" }]), "the agent spoke last: {v}");
    let (_, v) = call(&base, "a", "POST", room, Some(json!({ "text": "a chimes in", "parent": root }))).await;
    assert_eq!(v["pushed"].as_array().map(|p| p.iter().filter(|p| p["why"] == "agent").count()), Some(0), "{v}");
}
