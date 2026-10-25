//! How long `GET /api/chat` takes with a team's worth of rooms (M138's
//! latency check): channels, DMs and pane threads on the team's machines,
//! each with a few messages, listed by an editor who reads some of the
//! threads. Not a CI check (timings flake): run it by hand, on this commit
//! and on the one before a change to chat, and compare.
//!
//! `cargo test -p arugula-control --release -- --ignored --nocapture chat_bench`
//!
//! It runs twice: with the platform in the hub's process, and out of it
//! behind the hub's gateway (M140), where every request is forwarded and,
//! with the cache dropped, every hub answer comes over `HttpHub`.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_wire::chat::ChatReader;
use arugula_e2e::{now_ms, team::TeamRole};

use crate::{
    App,
    db::Team,
    routing_wire::{control, daemon, member, person, roster, session},
};

const TEAM: &str = "0123456789abcdef";
const MATES: usize = 20;
const MACHINES: usize = 10;
const CHANNELS: usize = 40;
const THREADS_PER_MACHINE: usize = 6;
const DMS: usize = 10;
const MESSAGES: usize = 3;
const ROUNDS: usize = 300;

/// The hub, where it's reached, and the platform chat's rows are in.
struct Stack {
    app: Arc<App>,
    base: String,
    http: reqwest::Client,
    platform: Arc<arugula_platform::Platform>,
}

async fn stack(out_of_process: bool) -> Stack {
    if out_of_process {
        let s = crate::gateway_wire::split().await;
        Stack { app: s.hub, base: s.base, http: reqwest::Client::new(), platform: s.platform }
    } else {
        let c = control(|_| {}).await;
        let platform = c.app.platform.inner().clone();
        Stack { app: c.app, base: c.base, http: c.http, platform }
    }
}

fn room(id: String, scope: String, kind: &str, machine: Option<String>, target: Option<String>) -> Room {
    Room {
        name: id.clone(),
        id,
        scope,
        kind: kind.into(),
        machine,
        target,
        created_by: "a0".into(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    }
}

use arugula_platform::db::chat::{NewMessage, Room};

#[tokio::test]
#[ignore = "a measurement, by hand: see the top of the file"]
async fn chat_bench() {
    for out_of_process in [false, true] {
        let said = bench(out_of_process).await;
        println!("chat_bench, platform {} process: {said}", if out_of_process { "out of" } else { "in" });
    }
}

async fn bench(out_of_process: bool) -> String {
    let c = stack(out_of_process).await;
    let mut keys = Vec::new();
    for i in 0..MATES {
        keys.push(person(&c.app, &format!("mate{i}"), &format!("a{i}")));
    }
    let members = (0..MATES)
        .map(|i| member(&format!("a{i}"), &keys[i], if i == 0 { TeamRole::Owner } else { TeamRole::Editor }))
        .collect();
    let r = roster(TEAM, 1, members, &keys[0]);
    let t =
        Team { id: TEAM.into(), name: "acme".into(), founder: "a0".into(), founder_root: keys[0].id(), locked: false };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    c.app.db.set_labs_flag(&format!("team:{TEAM}"), "chat", Some(true)).unwrap();
    let db = &c.platform.db;
    let mut rooms = Vec::new();
    for i in 0..CHANNELS {
        let r = room(format!("ch_{i}"), format!("team:{TEAM}"), "channel", None, None);
        db.add_chat_room(&r, &[], None).unwrap();
        rooms.push(r.id);
    }
    for i in 1..=DMS {
        let r = room(format!("dm_{i}"), format!("team:{TEAM}"), "dm", None, None);
        db.add_chat_room(&r, &["a1", &format!("a{}", i + 1)], None).unwrap();
        rooms.push(r.id);
    }
    for m in 0..MACHINES {
        let id = daemon(&c.app, "a0", &format!("box-{m}")).id();
        c.app.db.set_daemon_team(&id, TEAM).unwrap();
        for p in 0..THREADS_PER_MACHINE {
            let r = room(format!("th_{m}_{p}"), format!("machine:{id}"), "pane", Some(id.clone()), Some(p.to_string()));
            db.add_chat_room(&r, &[], None).unwrap();
            // a1 reads half of them.
            let readers: Vec<ChatReader> = if p % 2 == 0 {
                vec![ChatReader { account: "a1".into(), from: None, watching: false }]
            } else {
                vec![ChatReader { account: "a2".into(), from: None, watching: false }]
            };
            db.set_chat_readers(&id, "pane", &p.to_string(), &readers, now_ms()).unwrap();
            rooms.push(r.id);
        }
    }
    for r in &rooms {
        for n in 0..MESSAGES {
            let mentions = if n == 0 { vec!["a1".to_owned()] } else { vec![] };
            let m = NewMessage {
                author: "a0",
                at: now_ms(),
                text: "the mean test fails on box-3",
                quote: None,
                ref_machine: None,
                ref_pane: None,
                mentions: &mentions,
                agent: None,
                via: None,
                parent: None,
                also: false,
            };
            db.add_chat_message(r, &m).unwrap();
        }
    }
    let cookie = session(&c.app, "a1");
    let get = || async {
        let r = c.http.get(format!("{}/api/chat", c.base)).header("cookie", &cookie).send().await.unwrap();
        assert_eq!(r.status(), 200);
        r.json::<serde_json::Value>().await.unwrap()
    };
    let listed = get().await["rooms"].as_array().unwrap().len();
    for _ in 0..20 {
        get().await;
    }
    let measure = |cold: bool| {
        let get = &get;
        let platform = &c.platform;
        async move {
            let mut took: Vec<Duration> = Vec::with_capacity(ROUNDS);
            for _ in 0..ROUNDS {
                if cold {
                    // As after a change in the hub, or past the cache tick:
                    // every hub answer asked again.
                    platform.cache.drop_all();
                }
                let t = Instant::now();
                get().await;
                took.push(t.elapsed());
            }
            took.sort();
            let at = |q: f64| took[((took.len() as f64 * q) as usize).min(took.len() - 1)].as_secs_f64() * 1e3;
            format!("p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms", at(0.50), at(0.95), at(0.99))
        }
    };
    let warm = measure(false).await;
    let cold = measure(true).await;
    format!(
        "{} rooms in all, {listed} listed; GET /api/chat x{ROUNDS}: {warm}; with the hub's answers dropped before each: {cold}",
        rooms.len()
    )
}
