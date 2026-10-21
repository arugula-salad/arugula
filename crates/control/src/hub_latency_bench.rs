//! What a hub call costs a platform request when the hub is in another
//! region (platform step 2, #1150): the platform out of process behind the
//! hub's gateway, as in `chat_bench`, with every `HubPort` call over
//! `HttpHub` held back by [`DELAY`] first, about one ewr↔iad round trip.
//! Not a CI check (timings flake): run it by hand, on this commit and on
//! the one before a change to how the platform asks the hub, and compare.
//!
//! Three columns per route: **warm** (requests back to back), **every
//! 30 s** (the cache's clock moved on 30 s before each request, with the
//! change feed read throughout, as on a quiet control), and **cache
//! dropped** (everything asked again: what every 30 s cost before the
//! cache kept what the feed covers).
//!
//! `cargo test -p arugula-control --release -- --ignored --nocapture hub_latency_bench`

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_common::Principal;
use arugula_e2e::{now_ms, team::TeamRole};
use arugula_platform::{
    HubPort,
    db::chat::Room,
    hub::{Account, ChangePage, Daemon, Flags, HubResult, Meter, PushOptions, Pushed, Resolved, Team},
};
use futures_util::future::BoxFuture;

use crate::{
    db,
    routing_wire::{daemon, member, person, roster, session},
};

/// Added to every hub call: about what ewr↔iad adds over 6PN.
const DELAY: Duration = Duration::from_millis(7);
const ROUNDS: usize = 200;
const TEAM: &str = "0123456789abcdef";

/// A hub port that waits [`DELAY`] before each call.
struct Far(Arc<dyn HubPort>);

macro_rules! far {
    ($call:expr) => {
        Box::pin(async move {
            tokio::time::sleep(DELAY).await;
            $call.await
        })
    };
}

impl HubPort for Far {
    fn resolve<'a>(&'a self, p: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        far!(self.0.resolve(p))
    }
    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        far!(self.0.accounts(ids))
    }
    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        far!(self.0.accounts_called(word))
    }
    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.teams_of(account))
    }
    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        far!(self.0.teams(ids))
    }
    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        far!(self.0.daemons(ids))
    }
    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.machines_for(account))
    }
    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.may_reach(account, machines))
    }
    fn push<'a>(&'a self, account: &'a str, message: &'a serde_json::Value) -> BoxFuture<'a, HubResult<Pushed>> {
        far!(self.0.push(account, message))
    }
    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a serde_json::Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>> {
        far!(self.0.push_with(account, message, options))
    }
    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
        far!(self.0.labs_flags(owners))
    }
    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        far!(self.0.meters(accounts, since, until))
    }
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        far!(self.0.metered_accounts())
    }
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        far!(self.0.changes(after, limit))
    }
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        far!(self.0.personal_project(account))
    }
    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        far!(self.0.can_drive_cloud(daemon, account))
    }
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        far!(self.0.team_project(team))
    }
}

#[tokio::test]
#[ignore = "a measurement, by hand: see the top of the file"]
async fn hub_latency_bench() {
    let s = crate::gateway_wire::split_with(|hub| Arc::new(Far(hub))).await;
    // a0 owns team acme (five mates) and its five machines; chat is on.
    let keys: Vec<_> = (0..5).map(|i| person(&s.hub, &format!("mate{i}"), &format!("a{i}"))).collect();
    let members = (0..5)
        .map(|i| member(&format!("a{i}"), &keys[i], if i == 0 { TeamRole::Owner } else { TeamRole::Editor }))
        .collect();
    let r = roster(TEAM, 1, members, &keys[0]);
    let t = db::Team {
        id: TEAM.into(),
        name: "acme".into(),
        founder: "a0".into(),
        founder_root: keys[0].id(),
        locked: false,
    };
    s.hub.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    s.hub.db.set_labs_flag(&format!("team:{TEAM}"), "chat", Some(true)).unwrap();
    for m in 0..5 {
        let id = daemon(&s.hub, "a0", &format!("box-{m}")).id();
        s.hub.db.set_daemon_team(&id, TEAM).unwrap();
    }
    for i in 0..10 {
        let room = Room {
            id: format!("ch_{i}"),
            name: format!("ch_{i}"),
            scope: format!("team:{TEAM}"),
            kind: "channel".into(),
            machine: None,
            target: None,
            created_by: "a0".into(),
            created: now_ms(),
            archived: None,
            last_seq: 0,
        };
        s.platform.db.add_chat_room(&room, &[], None).unwrap();
    }
    // The platform's change-feed follower, as `arugula-platform` runs it.
    arugula_platform::chat::start(&s.platform);
    arugula_platform::cache::catch_up(&s.platform).await.unwrap();
    let cookie = session(&s.hub, "a0");
    let http = reqwest::Client::new();
    let get = |path: &'static str| {
        let (http, cookie, base) = (&http, &cookie, &s.base);
        async move {
            let r = http.get(format!("{base}{path}")).header("cookie", cookie).send().await.unwrap();
            assert_eq!(r.status(), 200, "{path}");
            r.bytes().await.unwrap();
        }
    };
    let mut lines =
        vec![format!("hub_latency_bench: {} ms added to every hub call, {ROUNDS} rounds", DELAY.as_millis())];
    for path in ["/api/me", "/api/grants", "/api/chat", "/api/billing", "/api/cloud/workspaces"] {
        for _ in 0..10 {
            get(path).await;
        }
        let mut said = format!("{path:24}");
        for (column, how) in [("warm", 0), ("every 30 s", 1), ("cache dropped", 2)] {
            let mut took = Vec::with_capacity(ROUNDS);
            for _ in 0..ROUNDS {
                match how {
                    1 => {
                        // 30 s on, and the feed read just now, as the
                        // follower does every second.
                        s.platform.cache.advance(Duration::from_secs(30));
                        arugula_platform::cache::catch_up(&s.platform).await.unwrap();
                    }
                    2 => s.platform.cache.drop_all(),
                    _ => {}
                }
                let t = Instant::now();
                get(path).await;
                took.push(t.elapsed());
            }
            took.sort();
            let at = |q: f64| took[((took.len() as f64 * q) as usize).min(took.len() - 1)].as_secs_f64() * 1e3;
            said += &format!("  {column}: p50 {:6.2} ms, p95 {:6.2} ms", at(0.5), at(0.95));
        }
        lines.push(said);
    }
    println!("{}", lines.join("\n"));
}
