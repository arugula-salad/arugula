//! Chat through the platform's own routes, with a hub of the test's own
//! ([`TestHub`]): what it asks the hub for a page of rooms, how a roster
//! change reaches it through the change feed, and how it pushes. Control's
//! `chat_wire` runs chat end to end through the hub's gate and in-process
//! port.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use arugula_control_common::Principal;
use arugula_control_wire::chat as wire;
use arugula_e2e::{now_ms, team::TeamRole};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat::FLAG,
    db::chat::Room,
    hub::{
        Account, Change, ChangeKind, ChangePage, Daemon, Flags, HubPort, HubResult, Member, Meter, PushOptions, Pushed,
        Resolved, Team,
    },
};

/// The hub as the platform asks it: accounts, teams, machines, shares and flags
/// set by the test, its change feed, the pushes it was asked for, and
/// how many times each method was called.
#[derive(Default)]
pub(crate) struct TestHub {
    state: Mutex<State>,
    calls: Mutex<BTreeMap<&'static str, usize>>,
    /// Its change feed fails while set.
    feed_down: AtomicBool,
    /// Its next feed page says changes were pruned unread.
    gap_next: AtomicBool,
    /// Its change feed takes 5 s to answer while set.
    pub(crate) feed_slow: AtomicBool,
}

#[derive(Default)]
struct State {
    accounts: BTreeSet<String>,
    teams: BTreeMap<String, Team>,
    daemons: BTreeMap<String, Daemon>,
    /// (machine, account) shares.
    shared: BTreeSet<(String, String)>,
    flags: BTreeMap<String, Flags>,
    changes: Vec<Change>,
    pushes: Vec<(String, Value, PushOptions)>,
    /// Accounts signed in with GitHub, by account.
    github: BTreeMap<String, crate::forge::GithubUser>,
}

fn done<'a, T: Send + 'a>(t: T) -> BoxFuture<'a, HubResult<T>> {
    Box::pin(async move { Ok(t) })
}

impl TestHub {
    fn called(&self, method: &'static str) {
        *self.calls.lock().unwrap().entry(method).or_default() += 1;
    }

    /// Calls of every method so far.
    pub(crate) fn calls(&self) -> usize {
        self.calls.lock().unwrap().values().sum()
    }

    /// Calls of each method so far.
    pub(crate) fn calls_by_method(&self) -> BTreeMap<&'static str, usize> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn calls_of(&self, method: &str) -> usize {
        self.calls.lock().unwrap().get(method).copied().unwrap_or(0)
    }

    pub(crate) fn change(&self, kind: ChangeKind, subject: &str) {
        let mut s = self.state.lock().unwrap();
        let seq = s.changes.len() as u64 + 1;
        s.changes.push(Change { seq, at: now_ms(), kind, subject: subject.into() });
    }

    /// A team's roster now (making the team, and its members' accounts).
    pub(crate) fn team(&self, id: &str, members: &[(&str, TeamRole)]) {
        {
            let mut s = self.state.lock().unwrap();
            s.accounts.extend(members.iter().map(|(a, _)| (*a).to_owned()));
            let version = s.teams.get(id).map_or(1, |t| t.version + 1);
            let locked = s.teams.get(id).is_some_and(|t| t.locked);
            let members = members
                .iter()
                .map(|(a, role)| Member { account: (*a).into(), role: *role, name: (*a).into() })
                .collect();
            let founder = s.teams.get(id).map_or_else(String::new, |t| t.founder.clone());
            s.teams.insert(id.into(), Team { id: id.into(), name: id.into(), founder, locked, version, members });
        }
        self.change(ChangeKind::TeamRoster, id);
    }

    pub(crate) fn daemon(&self, id: &str, account: &str, team: Option<&str>) {
        self.state.lock().unwrap().daemons.insert(
            id.into(),
            Daemon { id: id.into(), account: account.into(), team: team.map(Into::into), name: format!("{id}-box") },
        );
        self.change(ChangeKind::DaemonJoined, id);
    }

    /// The account signed in with GitHub as `login`, GitHub's user `id`.
    pub(crate) fn github(&self, account: &str, id: u64, login: &str) {
        let mut s = self.state.lock().unwrap();
        s.accounts.insert(account.into());
        s.github.insert(account.into(), crate::forge::GithubUser { id, login: login.into() });
    }

    /// A machine leaves its account.
    pub(crate) fn leave(&self, id: &str) {
        self.state.lock().unwrap().daemons.remove(id);
        self.change(ChangeKind::DaemonLeft, id);
    }

    /// Share a machine with an account, or stop.
    pub(crate) fn share(&self, machine: &str, account: &str, on: bool) {
        {
            let mut s = self.state.lock().unwrap();
            let k = (machine.to_owned(), account.to_owned());
            if on {
                s.shared.insert(k);
            } else {
                s.shared.remove(&k);
            }
        }
        self.change(ChangeKind::DaemonAccess, machine);
    }

    /// Its change feed fails from now on, or works again.
    pub(crate) fn feed_down(&self, down: bool) {
        self.feed_down.store(down, Ordering::SeqCst);
    }

    /// Its next feed page says changes were pruned before they were read.
    pub(crate) fn gap(&self) {
        self.gap_next.store(true, Ordering::SeqCst);
    }

    pub(crate) fn flag(&self, owner: &str, on: bool) {
        self.state.lock().unwrap().flags.entry(owner.into()).or_default().insert(FLAG.into(), on);
        self.change(ChangeKind::LabsFlag, owner);
    }

    pub(crate) fn pushes(&self) -> Vec<(String, Value, PushOptions)> {
        self.state.lock().unwrap().pushes.clone()
    }

    fn reaches(s: &State, account: &str, machine: &str) -> bool {
        let Some(d) = s.daemons.get(machine) else { return false };
        d.account == account
            || d.team.as_ref().and_then(|t| s.teams.get(t)).is_some_and(|t| t.role(account).is_some())
            || s.shared.contains(&(machine.to_owned(), account.to_owned()))
    }
}

impl HubPort for TestHub {
    fn resolve<'a>(&'a self, _: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        self.called("resolve");
        done(None)
    }
    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        self.called("accounts");
        let s = self.state.lock().unwrap();
        let found = ids
            .iter()
            .filter(|id| s.accounts.contains(*id))
            .map(|id| Account { id: id.clone(), login: id.clone(), name: id.clone() })
            .collect();
        done(found)
    }
    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        self.called("accounts_called");
        let s = self.state.lock().unwrap();
        let found = s
            .accounts
            .iter()
            .filter(|a| a.eq_ignore_ascii_case(word))
            .map(|a| Account { id: a.clone(), login: a.clone(), name: a.clone() })
            .collect();
        done(found)
    }
    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        self.called("teams_of");
        let s = self.state.lock().unwrap();
        let found = s.teams.values().filter(|t| t.members.iter().any(|m| m.account == account)).map(|t| t.id.clone());
        done(found.collect())
    }
    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        self.called("teams");
        let s = self.state.lock().unwrap();
        done(ids.iter().filter_map(|id| s.teams.get(id).cloned()).collect())
    }
    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        self.called("daemons");
        let s = self.state.lock().unwrap();
        done(ids.iter().filter_map(|id| s.daemons.get(id).cloned()).collect())
    }
    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        self.called("machines_for");
        let s = self.state.lock().unwrap();
        let found = s
            .daemons
            .values()
            .filter(|d| {
                d.account == account
                    || d.team
                        .as_ref()
                        .and_then(|t| s.teams.get(t))
                        .is_some_and(|t| t.members.iter().any(|m| m.account == account))
                    || s.shared.contains(&(d.id.clone(), account.to_owned()))
            })
            .map(|d| d.id.clone());
        done(found.collect())
    }
    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        self.called("may_reach");
        let s = self.state.lock().unwrap();
        done(machines.iter().filter(|m| Self::reaches(&s, account, m)).cloned().collect())
    }
    fn push<'a>(&'a self, account: &'a str, message: &'a Value) -> BoxFuture<'a, HubResult<Pushed>> {
        self.push_with(account, message, PushOptions::default())
    }
    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>> {
        self.called("push");
        self.state.lock().unwrap().pushes.push((account.into(), message.clone(), options));
        done(Pushed { sent: 1, gone: 0, failed: 0 })
    }
    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
        self.called("labs_flags");
        let s = self.state.lock().unwrap();
        done(owners.iter().filter_map(|o| Some((o.clone(), s.flags.get(o)?.clone()))).collect())
    }
    fn meters<'a>(&'a self, accounts: &'a [String], _: u64, _: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        self.called("meters");
        let meters =
            accounts.iter().map(|a| Meter { account: a.clone(), relay_bytes: 1024, sandbox_minutes: 3 }).collect();
        done(meters)
    }
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        self.called("metered_accounts");
        done(vec![])
    }
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        if self.feed_down.load(Ordering::SeqCst) {
            return Box::pin(async { Err(anyhow::anyhow!("the feed is down")) });
        }
        if self.feed_slow.load(Ordering::SeqCst) {
            return Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Err(anyhow::anyhow!("the feed was slow"))
            });
        }
        let gap = self.gap_next.swap(false, Ordering::SeqCst);
        let s = self.state.lock().unwrap();
        let changes = s.changes.iter().filter(|c| c.seq > after).take(limit as usize).cloned().collect();
        done(ChangePage { changes, last: s.changes.len() as u64, gap })
    }
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        self.called("personal_project");
        done(Some(format!("prj_{account}")))
    }
    fn can_drive_cloud<'a>(&'a self, _: &'a str, _: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        self.called("can_drive_cloud");
        done(false)
    }
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        self.called("team_project");
        let there = self.state.lock().unwrap().teams.contains_key(team);
        done(there.then(|| format!("prj_{team}")))
    }
    fn forge_pokes<'a>(&'a self, pokes: &'a [arugula_control_wire::forge::Poke]) -> BoxFuture<'a, HubResult<u32>> {
        self.called("forge_pokes");
        done(pokes.len() as u32)
    }

    fn github_user<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<crate::forge::GithubUser>>> {
        self.called("github_user");
        done(self.state.lock().unwrap().github.get(account).cloned())
    }
}

/// The platform over `hub`, its chat routes behind a test gate
/// (`ilg_session=session-<account>`), and its address.
pub(crate) async fn serve(hub: Arc<TestHub>) -> (Arc<Platform>, String) {
    serve_on(hub, crate::db::Db::test()).await
}

/// [`serve`] on `db`.
async fn serve_on(hub: Arc<TestHub>, db: crate::db::Db) -> (Arc<Platform>, String) {
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let app = Arc::new(Platform::new(cfg, db, hub, reqwest::Client::new(), Default::default()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (app, base)
}

pub(crate) async fn call(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{who}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

fn room(id: &str, scope: &str, kind: &str, machine: Option<&str>, target: Option<&str>) -> Room {
    Room {
        id: id.into(),
        scope: scope.into(),
        kind: kind.into(),
        name: id.into(),
        machine: machine.map(Into::into),
        target: target.map(Into::into),
        created_by: "a".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    }
}

/// a owns team t and its machines m0..m4, with six pane threads each;
/// b, an editor, reads the even ones. Chat is on for t.
pub(crate) fn seed(app: &Platform, hub: &TestHub) {
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
    hub.flag("team:t", true);
    app.db.add_chat_room(&room("ch_general", "team:t", "channel", None, None), &[], Some("prj_t")).unwrap();
    for m in 0..5 {
        let id = format!("m{m}");
        hub.daemon(&id, "a", Some("t"));
        for p in 0..6 {
            let target = p.to_string();
            let r = room(&format!("th_{m}_{p}"), &format!("machine:{id}"), "pane", Some(&id), Some(&target));
            app.db.add_chat_room(&r, &[], None).unwrap();
            let reader = if p % 2 == 0 { "b" } else { "a" };
            let readers = [wire::ChatReader { account: reader.into(), from: None, watching: false }];
            app.db.set_chat_readers(&id, "pane", &target, &readers, 1).unwrap();
        }
    }
}

/// Listing rooms asks the hub for a page of rooms at once: one
/// `may_reach` for every pane thread on others' machines, and nothing
/// at all again while nothing changed.
#[tokio::test]
async fn a_page_of_rooms_asks_the_hub_once_each() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    let (st, v) = call(&base, "b", "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let rooms = v["rooms"].as_array().unwrap();
    assert_eq!(rooms.len(), 1 + 5 * 3, "#general and the threads b reads");
    assert_eq!(rooms.iter().filter(|r| r["kind"] == "pane").count(), 15);
    assert_eq!(rooms[1]["machine_name"], "m0-box");
    for m in ["may_reach", "teams", "daemons", "machines_for"] {
        assert_eq!(hub.calls_of(m), 1, "{m}: one call for 30 threads on 5 machines");
    }
    let before = hub.calls();
    assert_eq!(call(&base, "b", "GET", "/api/chat", None).await.0, 200);
    assert_eq!(hub.calls(), before, "kept: nothing changed");
}

/// A roster change reaches who sees what once the feed is read, and not
/// before (the delay the docs give, `cache::DELAY`).
#[tokio::test]
async fn a_roster_change_reaches_chat_through_the_feed() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    // Chat stays on for b out of t.
    hub.flag("account:b", true);
    crate::cache::catch_up(&app).await.unwrap();
    let general = "/api/chat/rooms/ch_general";
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200);
    hub.team("t", &[("a", TeamRole::Owner)]);
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200, "until the feed is read");
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 404, "b left t");
    // And back.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200);
}

/// A mention goes through the hub's push, to every subscription the
/// account has, with the hub's TTL: what chat's pushes always did.
#[tokio::test]
async fn a_mention_pushes_through_the_hub_as_chat_always_did() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    let (st, v) =
        call(&base, "a", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "@b look" }))).await;
    assert_eq!((st, &v["pushed"]), (200, &json!([{ "account": "b", "why": "mention" }])), "{v}");
    let mut pushes = hub.pushes();
    for _ in 0..50 {
        if !pushes.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        pushes = hub.pushes();
    }
    let (to, msg, options) = pushes.first().cloned().expect("pushed");
    assert_eq!((to.as_str(), options), ("b", PushOptions { ttl: None, any_subscription: true }));
    assert_eq!(msg["title"], "a mentioned you in #ch_general");
    assert_eq!(
        (&msg["tag"], &msg["control"], &msg["chat"]["seq"]),
        (&json!("chat-ch_general"), &json!(true), &json!(1))
    );
}

/// A live stream's text so far, read in the background.
fn listen(mut r: reqwest::Response) -> Arc<Mutex<String>> {
    let heard = Arc::new(Mutex::new(String::new()));
    let into = heard.clone();
    tokio::spawn(async move {
        while let Ok(Some(chunk)) = r.chunk().await {
            into.lock().unwrap().push_str(&String::from_utf8_lossy(&chunk));
        }
    });
    heard
}

async fn open_live(base: &str, who: &str, last: Option<&str>) -> Arc<Mutex<String>> {
    let mut req = reqwest::Client::new()
        .get(format!("{base}/api/chat/live"))
        .header("cookie", format!("ilg_session=session-{who}"));
    if let Some(id) = last {
        req = req.header("last-event-id", id);
    }
    let r = req.send().await.unwrap();
    assert_eq!(r.status(), 200);
    listen(r)
}

/// Until `heard` has `what`, at most `within`; how long it took.
async fn hears(heard: &Mutex<String>, what: &str, within: Duration) -> Duration {
    let started = std::time::Instant::now();
    while !heard.lock().unwrap().contains(what) {
        assert!(started.elapsed() < within, "never heard {what}: {}", heard.lock().unwrap());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    started.elapsed()
}

/// Two machines on one Postgres (M144): a message posted through one
/// reaches a live stream held by the other, woken by `pg_notify` (well
/// within the catch-up that would find it anyway), and a page that dropped
/// from one resumes on the other from its `Last-Event-ID`.
#[tokio::test]
async fn a_message_posted_on_one_machine_reaches_a_stream_on_the_other() {
    let db = crate::db::Db::test();
    let Some(db_a) = db.second() else { return eprintln!("skipped: two machines need Postgres") };
    let db_b = db.second().unwrap();
    let hub = Arc::new(TestHub::default());
    let (a, base_a) = serve_on(hub.clone(), db_a).await;
    let (b, base_b) = serve_on(hub.clone(), db_b).await;
    crate::chat::start(&a);
    crate::chat::start(&b);
    seed(&a, &hub);

    let on_b = open_live(&base_b, "b", None).await;
    hears(&on_b, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let (st, v) =
        call(&base_a, "a", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "hi" }))).await;
    assert_eq!(st, 200, "{v}");
    let took = hears(&on_b, "\"type\":\"message\",\"room\":\"ch_general\",\"seq\":1", Duration::from_secs(4)).await;
    assert!(took < crate::chat::CATCH_UP, "woken by the notification, not the catch-up ({took:?})");

    // The page drops after that event, a second message is posted through
    // b, and the page comes back through a.
    let text = on_b.lock().unwrap().clone();
    let id = text.lines().filter_map(|l| l.strip_prefix("id: ")).next_back().expect("an event id").to_owned();
    assert!(id.starts_with("c-"), "{id}");
    let (st, _) =
        call(&base_b, "b", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "and" }))).await;
    assert_eq!(st, 200);
    let on_a = open_live(&base_a, "b", Some(&id)).await;
    hears(&on_a, "\"type\":\"message\",\"room\":\"ch_general\",\"seq\":2", Duration::from_secs(4)).await;
    assert!(!on_a.lock().unwrap().contains("resync"), "resumed, not resynced: {}", on_a.lock().unwrap());
    assert!(!on_a.lock().unwrap().contains("\"seq\":1}"), "not sent again");
    // An id from before M144 (one process's numbering) resyncs.
    let old = open_live(&base_a, "b", Some("19a2b3c4d5e-7")).await;
    hears(&old, "\"type\":\"resync\"", Duration::from_secs(4)).await;
}

/// One of two machines leads (M144): it alone applies the hub's feed to
/// projects and grants, and when its connection closes the other takes
/// over within a try or two.
#[tokio::test]
async fn one_machine_leads_and_the_other_takes_over() {
    let db = crate::db::Db::test();
    let Some(db_a) = db.second() else { return eprintln!("skipped: two machines need Postgres") };
    let hub = Arc::new(TestHub::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let a = Platform::new(cfg.clone(), db_a, hub.clone(), reqwest::Client::new(), Default::default());
    let b = Platform::new(cfg, db.second().unwrap(), hub.clone(), reqwest::Client::new(), Default::default());
    assert!(a.is_leader() && !b.is_leader(), "the first to start leads");
    assert!(!crate::projects::follow(&b).await.unwrap(), "only the leader follows");
    assert!(crate::projects::follow(&a).await.unwrap());
    drop(a);
    let started = std::time::Instant::now();
    while !b.is_leader() {
        assert!(started.elapsed() < crate::db::pg::LEADER_TRY * 3, "b never took over");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(crate::projects::follow(&b).await.is_ok());
}
