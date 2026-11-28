//! Arugula #1339: the daemon wakes an idle chief of staff, an agent block
//! that leads its person's staff room, with nobody typing in its pane: on
//! its person's post in staff (control tells the lead's machine, naming its
//! session) and on one of its watches firing (control's message for its
//! Claude Code session). With its machine away, control keeps both and
//! sends them all when the daemon is back; the block wakes once for them.
//! Against a fake control (the relay socket sends `agents.mention` and takes
//! the daemon's answers and turn reports) and a real daemon with a fake-ACP
//! agent block.
// `Daemon::stop` is Unix only, and so is every module that uses its helpers.
#![cfg(unix)]

use std::time::Duration;

use arugula_testkit::arugulad;
use serde_json::{Value, json};

use super::mention_agent_block::{Fake, answer, enroll, serve, states};

/// The staff room, as control names it.
const STAFF: &str = "dm_staff";

/// The person's post `seq` in their staff room, told to the session that
/// leads it.
fn staff_post(id: &str, seq: i64, text: &str) -> String {
    staff_post_to(id, seq, text, "Chief of staff", "Chief of staff")
}

/// [`staff_post`] in the room `room_name`, to the session `session`.
fn staff_post_to(id: &str, seq: i64, text: &str, room_name: &str, session: &str) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": STAFF, "room_name": room_name,
        "seq": seq, "author": "Jake", "text": text, "session": session, "from_owner": true } })
    .to_string()
}

/// Watch `watch`'s one message, for the Claude Code session `session`.
fn watch_fired(id: &str, watch: &str, session: &str, text: &str) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": watch, "room_name": "", "seq": 0,
        "author": "Arugula", "text": text, "watch": { "id": watch, "session": session } } })
    .to_string()
}

fn block_state(d: &arugula_testkit::Daemon, block: u64) -> Value {
    d.get(&format!("/api/blocks/{block}"))["state"].clone()
}

/// How many turns the block has begun.
fn turns(d: &arugula_testkit::Daemon, block: u64) -> u64 {
    block_state(d, block)["turns"].as_u64().unwrap_or(0)
}

/// The newest turn's prompt as the transcript has it: the last user entry.
fn last_prompt(d: &arugula_testkit::Daemon, block: u64) -> String {
    let st = block_state(d, block);
    let entries = st["entries"].as_array().cloned().unwrap_or_default();
    entries
        .iter()
        .rev()
        .find(|e| e["type"] == "user")
        .map(|e| e["text"].as_str().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

fn idle(d: &arugula_testkit::Daemon, block: u64) -> bool {
    let st = block_state(d, block);
    st["status"] == "ready" && st["queued"].as_array().is_none_or(Vec::is_empty)
}

/// A daemon enrolled with the fake control, and an agent block titled
/// "Chief of staff" whose first turn has ended: it sits idle.
fn boot(name: &str) -> (tokio::runtime::Runtime, Fake, arugula_testkit::Daemon, u64, std::path::PathBuf) {
    boot_with(name, json!({}))
}

/// [`boot`], with `extra` in the block's config.
fn boot_with(
    name: &str,
    extra: Value,
) -> (tokio::runtime::Runtime, Fake, arugula_testkit::Daemon, u64, std::path::PathBuf) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let sessions = std::env::temp_dir().join(format!("ilg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sessions);
    std::fs::create_dir_all(&sessions).unwrap();
    let mut d = arugulad!(name)
        .no_provider()
        .no_tailscale()
        .env("FAKE_ACP_DIR", &sessions)
        .env_remove("CLAUDE_CONFIG_DIR")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();

    let fake = format!("{}/../daemon/tests/fake_acp.py", env!("CARGO_MANIFEST_DIR"));
    let mut config = json!({ "agent": "acp", "command": ["python3", fake], "cwd": sessions, "title": "Chief of staff",
                         "prompt": "your standing instructions" });
    config.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let block = d.post("/api/blocks", json!({ "type": "agent", "config": config }))["block"].as_u64().unwrap();
    d.wait_for("its first turn to end", || turns(&d, block) == 1 && idle(&d, block));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    (rt, f, d, block, sessions)
}

#[test]
fn an_idle_chief_of_staff_wakes_on_its_persons_staff_post_and_on_its_watch() {
    let (_rt, f, d, block, sessions) = boot("coswakes");
    let session = block_state(&d, block)["session_id"].as_str().unwrap().to_owned();

    // (b) The person posts in staff: nobody types in the pane, and it's its
    // next turn at once.
    f.outbox.lock().unwrap().push(staff_post("p1", 5, "what's blocking launch?"));
    d.wait_for("the daemon's answer", || answer(&f, "p1").is_some());
    let a = answer(&f, "p1").unwrap();
    assert_eq!((&a["routed"]["step"], &a["routed"]["session"]), (&json!("named"), &json!("Chief of staff")), "{a}");
    d.wait_for("a second turn", || turns(&d, block) == 2);
    let prompt = last_prompt(&d, block);
    assert!(prompt.contains("Message from your person (Jake) in Chief of staff"), "{prompt}");
    assert!(prompt.contains("what's blocking launch?"), "{prompt}");
    d.wait_for("read, and idle again", || states(&f) == [(5, "working".into()), (5, "read".into())] && idle(&d, block));

    // (a) One of its watches fires: control's message names its Claude Code
    // session, and that is its next turn.
    f.outbox.lock().unwrap().push(watch_fired("w1", "wch_red", &session, "github.check.failed on main"));
    d.wait_for("the daemon's answer", || answer(&f, "w1").is_some());
    let a = answer(&f, "w1").unwrap();
    assert_eq!((&a["routed"]["step"], &a["routed"]["session"]), (&json!("named"), &json!("Chief of staff")), "{a}");
    d.wait_for("a third turn", || turns(&d, block) == 3);
    let prompt = last_prompt(&d, block);
    assert!(prompt.contains("Your watch wch_red (from Arugula, not a person"), "{prompt}");
    assert!(prompt.contains("github.check.failed on main"), "{prompt}");
    d.wait_for("idle again", || idle(&d, block));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(turns(&d, block), 3, "one turn for each wake");

    drop(d);
    let _ = std::fs::remove_dir_all(&sessions);
}

/// A chief of staff's session goes by its project's id (Arugula #1457),
/// apart from its block's title: its project renamed since it started (the
/// room's name, and the title it was given then, now differ), the person's
/// post in staff still reaches it by that session.
#[test]
fn a_chief_of_staff_of_a_renamed_project_still_gets_its_staff_posts() {
    let session = "Chief of staff · prj_1";
    let extra = json!({ "title": "Chief of staff · Launch crew", "session": session });
    let (_rt, f, d, block, sessions) = boot_with("cosrenamed", extra);
    let pane = d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == block).cloned().unwrap();
    assert_eq!(pane["title"], "Chief of staff · Launch crew", "people read the project's name: {pane}");

    f.outbox.lock().unwrap().push(staff_post_to("p1", 5, "still there?", "Chief of staff · Renamed crew", session));
    d.wait_for("the daemon's answer", || answer(&f, "p1").is_some());
    let a = answer(&f, "p1").unwrap();
    assert_eq!((&a["routed"]["step"], &a["routed"]["session"]), (&json!("named"), &json!(session)), "{a}");
    d.wait_for("a second turn", || turns(&d, block) == 2);
    assert!(last_prompt(&d, block).contains("still there?"));

    drop(d);
    let _ = std::fs::remove_dir_all(&sessions);
}

/// A board's lead (Arugula #1492): control names the lead's pane with the
/// session's old name. The session was renamed since, so the name finds
/// nothing, and the pane still takes the post. A pane that's gone leaves it
/// to the name, as before.
#[test]
fn a_board_threads_post_follows_its_leads_pane_not_its_old_name() {
    let (_rt, f, d, block, sessions) = boot("coslead");
    let lead_post = |id: &str, seq: i64, session: &str, pane: u64| {
        json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "brd_1", "room_name": "One Inbox",
            "seq": seq, "author": "Jake", "text": "status?", "session": session, "pane": pane, "from_owner": true } })
        .to_string()
    };

    // Led as "Rail and Machines"; the session answers to "Chief of staff" now.
    f.outbox.lock().unwrap().push(lead_post("l1", 5, "Rail and Machines", block));
    d.wait_for("the daemon's answer", || answer(&f, "l1").is_some());
    let a = answer(&f, "l1").unwrap();
    assert_eq!((&a["routed"]["step"], &a["routed"]["session"]), (&json!("named"), &json!("Chief of staff")), "{a}");
    d.wait_for("a second turn", || turns(&d, block) == 2);
    assert!(last_prompt(&d, block).contains("status?"));
    d.wait_for("idle again", || idle(&d, block));

    // The pane is gone (a restart gave the session another): the name does it.
    f.outbox.lock().unwrap().push(lead_post("l2", 6, "Chief of staff", 9999));
    d.wait_for("the daemon's answer", || answer(&f, "l2").is_some());
    let a = answer(&f, "l2").unwrap();
    assert_eq!((&a["routed"]["step"], &a["routed"]["session"]), (&json!("named"), &json!("Chief of staff")), "{a}");
    d.wait_for("a third turn", || turns(&d, block) == 3);

    // Neither: the inbox, as before.
    d.wait_for("idle again", || idle(&d, block));
    f.outbox.lock().unwrap().push(lead_post("l3", 7, "Rail and Machines", 9999));
    d.wait_for("the daemon's answer", || answer(&f, "l3").is_some());
    assert_eq!(answer(&f, "l3").unwrap()["routed"]["step"], "inbox");

    drop(d);
    let _ = std::fs::remove_dir_all(&sessions);
}

/// Its machine was away: control kept the person's posts in staff and a
/// watch's message, and sends them all at once when the daemon is back (as
/// its `redeliver` does, concurrently). The chief of staff wakes once, with
/// every one of them in that one prompt, each reported working then read.
#[test]
fn a_chief_of_staff_back_from_away_wakes_once_for_everything_kept() {
    let (_rt, f, mut d, block, sessions) = boot("cosaway");
    let session = block_state(&d, block)["session_id"].as_str().unwrap().to_owned();

    // Away: the daemon is gone, and the person goes on writing in staff.
    d.stop();
    f.said.lock().unwrap().clear();
    let kept = [
        staff_post("p1", 5, "are you there?"),
        staff_post("p2", 6, "what's blocking launch?"),
        watch_fired("w1", "wch_red", &session, "github.check.failed on main"),
        staff_post("p3", 7, "and the date?"),
    ];

    // Back: control sends what it kept as soon as the daemon is on the relay.
    d.start();
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let before = turns(&d, block);
    f.outbox.lock().unwrap().extend(kept);
    d.wait_for("every answer", || ["p1", "p2", "w1", "p3"].iter().all(|id| answer(&f, id).is_some()));
    for id in ["p1", "p2", "w1", "p3"] {
        let a = answer(&f, id).unwrap();
        assert_eq!((&a["answer"], &a["routed"]["step"]), (&json!("done"), &json!("named")), "{id}: {a}");
    }

    d.wait_for("each read", || {
        let s = states(&f);
        [5, 6, 0, 7].iter().all(|seq| s.contains(&(*seq, "read".to_owned())))
    });
    d.wait_for("idle again", || idle(&d, block));
    std::thread::sleep(Duration::from_millis(1500));
    let st = block_state(&d, block);
    assert_eq!(turns(&d, block), before + 1, "woken once: {:?} {}", states(&f), st["recent_turns"]);
    let prompt = last_prompt(&d, block);
    for said in ["are you there?", "what's blocking launch?", "github.check.failed on main", "and the date?"] {
        assert!(prompt.contains(said), "{said} is in the one prompt: {prompt}");
    }
    let s = states(&f);
    assert_eq!(s.len(), 8, "each working, then read, once: {s:?}");

    drop(d);
    let _ = std::fs::remove_dir_all(&sessions);
}
