//! A daemon tells control who reads each pane's and session's thread while
//! control has chat on for its account or team, and says nothing while
//! it's off: chat's on/off is control's, not a Labs flag (native #203).
//! The threads an older daemon kept itself (M61) are no longer brought
//! into control: they stay on disk, untouched. A change in who may read
//! what reaches control at once, not at the next refresh, and a team's
//! other owners read every pane's thread, as they read the pane (native
//! #684), as a hosted workspace's editors do (native #705).

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_control_wire::hosted::{EnrollRequest, Enrollment};
use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    hosted::{Delegation, HostedCert, HostedKey},
    team::{AccountCerts, Member, Roster, TeamLock, TeamPin, TeamRole},
};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{RawQuery, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

/// A team as the fake control serves it: its rosters, oldest first, and
/// its members' certificates.
type Team = (Vec<Roster>, AccountCerts);

/// Whether control says a team is locked, and its owner's signed word on it.
type Locked = (bool, Option<TeamLock>);

/// A control that records what a daemon tells it.
#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The features each refresh named.
    features: Arc<Mutex<Vec<String>>>,
    /// Control's chat setting for the machine's team, sent with the flags.
    chat: Arc<Mutex<bool>>,
    /// Instead: chat heard from the owner's teams, a personal machine's word
    /// (native #673).
    heard: Arc<Mutex<bool>>,
    /// The account's own word for Doc copies, sent with its flags (native
    /// #673), and each Doc a machine put.
    copies: Arc<Mutex<bool>>,
    doc_puts: Arc<Mutex<Vec<String>>>,
    /// The account's own certificates, as the trust route answers.
    certs: Arc<Mutex<Vec<Cert>>>,
    /// A team machine's team: its rosters and its members' certificates.
    team: Arc<Mutex<Option<Team>>>,
    /// Whether control says that team is locked, and its owner's signed
    /// word on it.
    locked: Arc<Mutex<Locked>>,
    /// Teams sessions were shared with, by id: each one's team, and
    /// whether control says it's locked with its owner's signed word.
    teams: Arc<Mutex<std::collections::BTreeMap<String, (Team, Locked)>>>,
    /// Any call to the import route of M124, which is gone.
    imports: Arc<Mutex<Vec<Value>>>,
    /// Each reader list posted, and when.
    readers: Arc<Mutex<Vec<(Instant, Value)>>>,
    /// Refuse reader lists, as a control the machine can't reach would,
    /// and how many it refused.
    refuse: Arc<Mutex<bool>>,
    refused: Arc<Mutex<u32>>,
    /// A hosted machine's project (native #705): the team that owns it,
    /// with the roster and certificates in `team`.
    hosted: Arc<Mutex<Option<HostedProject>>>,
    /// How many times a hosted box asked for its trust: its refreshes.
    hosted_refreshes: Arc<Mutex<u32>>,
}

/// A team's project with a Cloud workspace, as the fake control enrolls
/// its box: the delegation (signed by the team's founder) and the key it
/// signs the box's certificate with.
#[derive(Clone)]
struct HostedProject {
    key: Arc<HostedKey>,
    delegation: Delegation,
    pin: TeamPin,
    /// The box's certificate, once it enrolled.
    cert: Option<HostedCert>,
}

impl Fake {
    /// What a hosted box pins and serves by: its certificate and the
    /// team's roster as it is now.
    fn enrollment(&self) -> Option<Enrollment> {
        let h = self.hosted.lock().unwrap().clone()?;
        let (rosters, certs) = self.team.lock().unwrap().clone().unwrap_or_default();
        Some(Enrollment {
            url: self.url.lock().unwrap().clone(),
            cert: h.cert?,
            delegation: h.delegation,
            revocations: vec![],
            closed: vec![],
            team: Some(h.pin),
            roster: rosters.last().cloned(),
            certs,
            locked: false,
            lock: None,
            trust: None,
            account_certs: vec![],
            account_revocations: vec![],
            legacy_app_children: None,
            passkey_logs: None,
            passkey_origins: None,
            purpose: None,
        })
    }
}

async fn hosted_enroll(State(f): State<Fake>, Json(b): Json<EnrollRequest>) -> Response {
    let Some(h) = f.hosted.lock().unwrap().clone() else { return StatusCode::NOT_FOUND.into_response() };
    let mut cert = HostedCert::new(&h.delegation, "hw_1", "box", &b.noise, &b.sign, arugula_e2e::now_ms());
    h.key.sign(&mut cert);
    f.hosted.lock().unwrap().as_mut().unwrap().cert = Some(cert);
    Json(f.enrollment().unwrap()).into_response()
}

async fn hosted_trust(State(f): State<Fake>) -> Response {
    *f.hosted_refreshes.lock().unwrap() += 1;
    match f.enrollment() {
        Some(e) => Json(e).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

fn query(q: Option<String>, key: &str) -> Option<String> {
    url::form_urlencoded::parse(q.unwrap_or_default().as_bytes()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

async fn trust(State(f): State<Fake>, RawQuery(q): RawQuery) -> Json<Value> {
    f.features.lock().unwrap().push(query(q, "features").unwrap_or_default());
    let team = if *f.heard.lock().unwrap() {
        json!({ "chat-heard": true })
    } else {
        json!({ "chat": *f.chat.lock().unwrap() })
    };
    let certs = f.certs.lock().unwrap().clone();
    let account = if *f.copies.lock().unwrap() { json!({ "doc-copies": true }) } else { json!({}) };
    Json(json!({ "certs": certs, "revocations": [], "flags": { "account": account, "team": team } }))
}

async fn doc_put(State(f): State<Fake>, axum::extract::Path(doc): axum::extract::Path<String>) -> Json<Value> {
    f.doc_puts.lock().unwrap().push(doc.clone());
    Json(json!({ "project": "prj_1", "id": format!("pdc_{doc}"), "link": "https://app.test/p/prj_1", "rev": 1 }))
}

async fn team(State(f): State<Fake>, RawQuery(q): RawQuery) -> Json<Value> {
    let since: u64 = query(q, "since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let (rosters, certs) = f.team.lock().unwrap().clone().unwrap_or_default();
    let rosters: Vec<Roster> = rosters.into_iter().filter(|r| r.version > since).collect();
    let (locked, lock) = f.locked.lock().unwrap().clone();
    Json(json!({ "locked": locked, "lock": lock, "rosters": rosters, "certs": certs }))
}

async fn teams(State(f): State<Fake>, RawQuery(q): RawQuery) -> Json<Value> {
    let ids = query(q, "ids").unwrap_or_default();
    let teams = f.teams.lock().unwrap();
    let mut out = serde_json::Map::new();
    for id in ids.split(',') {
        if let Some(((rosters, certs), (locked, lock))) = teams.get(id) {
            out.insert(id.into(), json!({ "locked": locked, "lock": lock, "rosters": rosters, "certs": certs }));
        }
    }
    Json(Value::Object(out))
}

async fn import(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.imports.lock().unwrap().push(b);
    Json(json!({ "rooms": 0, "messages": 0, "already": 0 }))
}

async fn readers(State(f): State<Fake>, Json(b): Json<Value>) -> Response {
    if *f.refuse.lock().unwrap() {
        *f.refused.lock().unwrap() += 1;
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let n = b["targets"].as_array().map_or(0, Vec::len);
    f.readers.lock().unwrap().push((Instant::now(), b));
    Json(json!({ "stored": n })).into_response()
}

async fn dial(up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        std::future::pending::<()>().await;
        drop(ws);
    })
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(about))
            .route("/api/daemon/trust", get(trust))
            .route("/api/daemon/team", get(team))
            .route("/api/daemon/teams", get(teams))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/daemon/chat/import", post(import))
            .route("/api/daemon/chat/readers", post(readers))
            .route("/api/daemon/docs/{doc}", axum::routing::put(doc_put))
            .route("/api/hosted/enroll", post(hosted_enroll))
            .route("/api/hosted/trust", get(hosted_trust))
            .route("/api/relay/dial", get(dial))
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "jake" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

/// The threads an older daemon kept (M61): one pane's.
fn old_threads(state: &std::path::Path) {
    let dir = state.join("threads");
    std::fs::create_dir_all(&dir).unwrap();
    let m = json!({ "id": 1, "at": 1000, "who": "owner", "name": "Jake", "text": "the mean test fails" });
    std::fs::write(dir.join("pane-4.jsonl"), format!("{m}\n")).unwrap();
}

/// The reader lists control was given for `kind` `target`, oldest first,
/// each as its readers.
fn lists_for(f: &Fake, kind: &str, target: &str) -> Vec<Value> {
    f.readers
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(_, b)| b["targets"].as_array().cloned().unwrap_or_default())
        .filter(|t| t["kind"] == kind && t["target"] == target)
        .map(|t| t["readers"].clone())
        .collect()
}

/// The accounts in the newest reader list for `kind` `target`, in order.
fn newest(f: &Fake, kind: &str, target: &str) -> Option<Vec<String>> {
    let list = lists_for(f, kind, target).pop()?;
    Some(list.as_array().unwrap().iter().map(|r| r["account"].as_str().unwrap().to_owned()).collect())
}

/// The panes the newest send said the machine holds.
fn held_panes(f: &Fake) -> Vec<String> {
    let r = f.readers.lock().unwrap();
    let held = r.iter().rev().find_map(|(_, b)| b.get("held").cloned()).unwrap_or_default();
    held["panes"].as_array().map(|a| a.iter().map(|p| p.as_str().unwrap().to_owned()).collect()).unwrap_or_default()
}

/// How long ago control was last given a reader list.
fn since_last(f: &Fake) -> Duration {
    f.readers.lock().unwrap().last().map_or(Duration::MAX, |(at, _)| at.elapsed())
}

/// The owner makes a pane private, or not, as the pane menu does.
fn set_private(d: &arugula_testkit::Daemon, pane: u64, on: bool) {
    use tokio_tungstenite::tungstenite::{self, Message};
    let stream = std::net::TcpStream::connect(("127.0.0.1", d.port)).unwrap();
    let (mut ws, _) = tungstenite::client(d.ws("/ws"), stream).expect("the WebSocket");
    let op = json!({ "type": "pane", "pane": pane, "op": { "op": "set_private", "on": on } });
    ws.send(Message::Text(op.to_string().into())).unwrap();
    let _ = ws.close(None);
    while ws.read().is_ok() {}
}

#[test]
fn reader_lists_follow_controls_chat_setting_and_old_threads_stay_put() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("chatreaders")
        .no_provider()
        .no_tailscale()
        .env_remove("ARUGULA_FEATURES")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    old_threads(&d.state);
    enroll(&d.state, &url);
    d.start();
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    let said = f.features.lock().unwrap()[0].clone();
    assert!(!said.split(',').any(|x| x == "chat-import"), "{said}: it brings nothing in");

    // Sharing a session with alex's account tells control he reads its
    // thread, and with carol's as a viewer that she only watches it (she
    // reads, and doesn't post).
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    d.post(
        "/api/acl",
        json!({ "session": session, "principal": "account:a2", "name": "alex", "role": "editor", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    d.post(
        "/api/acl",
        json!({ "session": session, "principal": "account:a4", "name": "carol", "role": "viewer", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    let list = json!([
        { "account": "a2", "from": null },
        { "account": "a4", "from": null, "watching": true },
    ]);
    let target = session.to_string();
    d.wait_for("alex's and carol's reader list", || lists_for(&f, "session", &target).contains(&list));

    // Taking alex's share away reaches control at once (native #684): well
    // before the next refresh, a minute off, and in one send for the whole
    // machine, not one per change.
    let revoked = Instant::now();
    d.post("/api/acl", json!({ "session": session, "principal": "account:a2", "role": null }));
    d.wait_for("carol alone", || newest(&f, "session", &target).as_deref() == Some(&["a4".to_owned()]));
    let took = revoked.elapsed();
    assert!(took < Duration::from_secs(25), "a revoked share waited {took:?} for control to hear");
    let sends = f.readers.lock().unwrap().len();
    assert!(sends <= 3, "{sends} sends for a start, two shares and a revoke");

    // Chat off in control: a new start, which would send them all again
    // with chat on, sends nothing.
    *f.chat.lock().unwrap() = false;
    d.stop();
    let (refreshes, sent) = (f.features.lock().unwrap().len(), f.readers.lock().unwrap().len());
    d.start();
    d.wait_for("a refresh", || f.features.lock().unwrap().len() > refreshes);
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(f.readers.lock().unwrap().len(), sent, "chat is off for it in control");

    // Heard from the owner's teams (a personal machine, native #673): the
    // platform takes reader lists for it, so they go again.
    *f.heard.lock().unwrap() = true;
    d.stop();
    let sent = f.readers.lock().unwrap().len();
    d.start();
    d.wait_for("the lists sent on heard chat", || f.readers.lock().unwrap().len() > sent);

    // The old threads were never sent anywhere, and stay where they were.
    assert!(f.imports.lock().unwrap().is_empty());
    assert!(d.state.join("threads/pane-4.jsonl").exists() && !d.state.join("threads.imported").exists());
}

/// A closed pane's thread keeps the readers it would have now: the
/// machine goes on holding it (across a restart too), a private one stays
/// its owner's, and a share of its session taken away later reaches
/// control. A share taken away while
/// control refuses the lists reaches it once control takes them again,
/// without waiting for a refresh, and that send names every list.
#[test]
fn a_closed_panes_readers_and_a_revoke_during_a_failed_send_reach_control() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    // No `--direct-url`, for `set_private` (see the co-owner test).
    let mut d =
        arugulad!("closedreaders").no_provider().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());

    let first = &d.get("/api/panes")[0];
    let (closing, sid) = (first["id"].as_u64().unwrap(), first["session"].as_u64().unwrap());
    let other = d.post("/api/run", json!({ "command": "sh", "session": sid.to_string() }))["pane"].as_u64().unwrap();
    let secret = d.post("/api/run", json!({ "command": "sh", "session": sid.to_string() }))["pane"].as_u64().unwrap();
    let (closing_t, other_t, session) = (closing.to_string(), other.to_string(), sid.to_string());
    let secret_t = secret.to_string();
    d.post(
        "/api/acl",
        json!({ "session": sid, "principal": "account:a2", "name": "alex", "role": "editor", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    let alex = Some(vec!["a2".to_owned()]);
    d.wait_for("alex reads the panes", || {
        newest(&f, "pane", &closing_t) == alex
            && newest(&f, "pane", &other_t) == alex
            && newest(&f, "pane", &secret_t) == alex
    });

    // One made private, then closed: held, and nobody's but the owner's.
    set_private(&d, secret, true);
    d.wait_for("the private pane the owner's", || newest(&f, "pane", &secret_t) == Some(vec![]));
    d.post(&format!("/api/panes/{secret}/close"), json!({}));

    // The pane closes: still held, and alex still reads its thread.
    d.post(&format!("/api/panes/{closing}/close"), json!({}));
    d.wait_for("the closed pane held", || {
        held_panes(&f).contains(&closing_t)
            && !d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == closing)
    });
    assert_eq!(newest(&f, "pane", &closing_t), alex);

    // Held across a restart.
    d.stop();
    let sent = f.readers.lock().unwrap().len();
    d.start();
    d.wait_for("the lists after a restart", || f.readers.lock().unwrap().len() > sent);
    let held = held_panes(&f);
    assert!(held.contains(&closing_t) && held.contains(&secret_t), "forgotten on restart: {held:?}");
    // That first send names every list, worked out again.
    assert_eq!((newest(&f, "pane", &closing_t), newest(&f, "pane", &secret_t)), (alex.clone(), Some(vec![])));

    // Control refuses the lists; alex's share is taken away meanwhile.
    *f.refuse.lock().unwrap() = true;
    let sent = f.readers.lock().unwrap().len();
    d.post("/api/acl", json!({ "session": sid, "principal": "account:a2", "role": null }));
    d.wait_for("a refused send", || *f.refused.lock().unwrap() > 0);
    assert_eq!(f.readers.lock().unwrap().len(), sent, "control took nothing");
    assert_eq!(newest(&f, "pane", &closing_t), alex, "control still has what it last took");
    *f.refuse.lock().unwrap() = false;
    let back = Instant::now();
    d.wait_for("alex in no list", || {
        newest(&f, "pane", &closing_t) == Some(vec![])
            && newest(&f, "pane", &other_t) == Some(vec![])
            && newest(&f, "session", &session) == Some(vec![])
    });
    assert!(back.elapsed() < Duration::from_secs(30), "the retry waited {:?}", back.elapsed());
    assert!(held_panes(&f).contains(&closing_t));

    // Shared the session again: the open pane is alex's, the closed one
    // stays not, here and after a restart.
    d.post(
        "/api/acl",
        json!({ "session": sid, "principal": "account:a2", "name": "alex", "role": "editor", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    d.wait_for("alex on the open pane again", || newest(&f, "pane", &other_t) == alex);
    assert_eq!(newest(&f, "pane", &closing_t), Some(vec![]), "a share again brought back the closed pane");
    d.stop();
    let sent = f.readers.lock().unwrap().len();
    d.start();
    d.wait_for("the lists after a restart", || f.readers.lock().unwrap().len() > sent);
    assert_eq!((newest(&f, "pane", &closing_t), newest(&f, "pane", &other_t)), (Some(vec![]), alex));
}

fn device(account: &str) -> (DeviceKeys, Cert) {
    let keys = DeviceKeys::generate();
    let mut cert = Cert::new(&keys, account, Kind::Browser, "x");
    cert.sign_with(&keys);
    (keys, cert)
}

fn member(account: &str, root: &Cert, role: TeamRole) -> Member {
    Member { account: account.into(), root: root.device.clone(), role, name: account.into() }
}

/// Team `t1`'s roster at `version`, with `members`, signed by its founder.
fn roster(founder: &DeviceKeys, version: u64, members: Vec<Member>) -> Roster {
    let mut r = Roster {
        v: 1,
        team: "t1".into(),
        name: "Acme".into(),
        version,
        at: version,
        members,
        spent: vec![],
        redeem: None,
        by: String::new(),
        sig: String::new(),
    };
    r.sign_with(founder);
    r
}

/// Alex (`a1`) owns the machine, in team `t1` with Bea (`a2`) as its other
/// owner and Cy (`a3`) as an editor: Bea reads every pane's thread, a
/// private one included, as she reads the pane; Cy reads what an editor
/// may. Both lists change the moment the pane is made private, and Bea
/// goes from them when the roster takes her out (native #684).
#[test]
fn a_teams_other_owner_reads_every_panes_thread_until_the_roster_takes_them_out() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    // The enrolling member's own word for copies, which a team's machine
    // never takes (native #673).
    *f.copies.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, bea_root) = device("a2");
    let (_, cy_root) = device("a3");
    let owners = |bea: Option<TeamRole>| {
        let mut m = vec![member("a1", &alex_root, TeamRole::Owner)];
        m.extend(bea.map(|r| member("a2", &bea_root, r)));
        m.push(member("a3", &cy_root, TeamRole::Editor));
        m
    };
    let mut certs = AccountCerts::new();
    for (a, c) in [("a1", &alex_root), ("a2", &bea_root), ("a3", &cy_root)] {
        certs.insert(a.into(), (vec![c.clone()], vec![]));
    }
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 1, owners(Some(TeamRole::Owner)))], certs.clone()));

    // No `--direct-url`: with one, the daemon's own port counts as public
    // and refuses the owner's WebSocket (`set_private` below) without a
    // tailnet identity.
    let mut d =
        arugulad!("coownerreaders").no_provider().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
    d.stop();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut daemon_cert = Cert::new(&daemon, "a1", Kind::Daemon, "box");
    daemon_cert.sign_with(&alex);
    *f.certs.lock().unwrap() = vec![alex_root.clone(), daemon_cert.clone()];
    let saved = json!({
        "url": url, "trust": { "account": "a1", "root": alex_root.device }, "cert": daemon_cert,
        "certs": [alex_root], "revocations": [], "login": "alex",
        "team": TeamPin { team: "t1".into(), founder: "a1".into(), founder_root: alex_root.device.clone() },
    });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.start();

    let p = &d.get("/api/panes")[0];
    let (pane, session) = (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap().to_string());
    let pane_t = pane.to_string();
    // Bea as an owner reads from the start and isn't watching; Cy by his
    // team role. Only accounts the roster names, and never the machine's own.
    let both = json!([{ "account": "a2", "from": null }, { "account": "a3", "from": null }]);
    d.wait_for("the pane's readers", || lists_for(&f, "pane", &pane_t).contains(&both));
    d.wait_for("the session's readers", || lists_for(&f, "session", &session).contains(&both));

    // Private: Cy loses the pane's thread at once, Bea keeps it (she reads
    // the pane, as the owner does), and the session is theirs as before.
    set_private(&d, pane, true);
    let changed = Instant::now();
    d.wait_for("bea alone on the private pane", || newest(&f, "pane", &pane_t).as_deref() == Some(&["a2".to_owned()]));
    assert!(changed.elapsed() < Duration::from_secs(25), "a private pane waited for a refresh");
    assert_eq!(newest(&f, "session", &session).unwrap(), ["a2", "a3"]);

    // The roster takes Bea out: once the daemon has it (a share nudges it
    // to look), she's in no list, and Cy still reads the session.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 2, owners(None))], certs.clone()));
    d.post(
        "/api/acl",
        json!({ "session": session.parse::<u64>().unwrap(), "principal": "account:a9", "name": "dee",
                "role": "viewer", "history": true, "root": DeviceKeys::generate().id() }),
    );
    d.wait_for("bea gone from the session", || {
        newest(&f, "session", &session).is_some_and(|l| !l.iter().any(|a| a == "a2") && l.iter().any(|a| a == "a3"))
    });
    assert_eq!(newest(&f, "pane", &pane_t).unwrap(), Vec::<String>::new(), "a private pane: its owner's alone");
    // A former owner demoted (not removed) would be an editor, like Cy.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 3, owners(Some(TeamRole::Editor)))], certs));
    d.post("/api/acl", json!({ "session": session.parse::<u64>().unwrap(), "principal": "account:a9", "role": null }));
    d.wait_for("bea back as an editor", || {
        newest(&f, "session", &session).as_deref() == Some(&["a2".into(), "a3".into()])
    });
    assert_eq!(newest(&f, "pane", &pane_t).unwrap(), Vec::<String>::new(), "an editor doesn't read a private pane");
    assert!(since_last(&f) < Duration::from_secs(60));

    // A team's machine copies no Doc up, whoever's word comes with the
    // flags (native #673).
    let block =
        d.post("/api/blocks", json!({ "type": "doc", "config": { "title": "Team notes" }, "local": true }))["block"]
            .as_u64()
            .unwrap();
    d.post(
        &format!("/api/blocks/{block}/call/doc_outline"),
        json!({ "title": "Team notes", "sections": [{ "heading": "One", "intent": "" }] }),
    );
    std::thread::sleep(Duration::from_secs(6));
    assert!(f.doc_puts.lock().unwrap().is_empty(), "a team's machine copied: {:?}", f.doc_puts.lock().unwrap());
}

/// A closed pane's thread only narrows (Jake, 2026-11-05). Alex (`a1`)
/// owns the machine in team `t1`, with Cy (`a3`) an editor. A pane
/// closes while Cy reads it: Dee (`a5`) joining the team, and a share of
/// the session with Eve (`a9`), give them the session's open pane and not
/// the closed one; the roster taking Cy out takes the closed one from him.
#[test]
fn a_closed_panes_readers_only_narrow() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, cy_root) = device("a3");
    let (_, dee_root) = device("a5");
    let members = |cy: bool, dee: bool| {
        let mut m = vec![member("a1", &alex_root, TeamRole::Owner)];
        if cy {
            m.push(member("a3", &cy_root, TeamRole::Editor));
        }
        if dee {
            m.push(member("a5", &dee_root, TeamRole::Editor));
        }
        m
    };
    let mut certs = AccountCerts::new();
    for (a, c) in [("a1", &alex_root), ("a3", &cy_root), ("a5", &dee_root)] {
        certs.insert(a.into(), (vec![c.clone()], vec![]));
    }
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 1, members(true, false))], certs.clone()));

    let mut d =
        arugulad!("narrowreaders").no_provider().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
    d.stop();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut daemon_cert = Cert::new(&daemon, "a1", Kind::Daemon, "box");
    daemon_cert.sign_with(&alex);
    *f.certs.lock().unwrap() = vec![alex_root.clone(), daemon_cert.clone()];
    let saved = json!({
        "url": url, "trust": { "account": "a1", "root": alex_root.device }, "cert": daemon_cert,
        "certs": [alex_root], "revocations": [], "login": "alex",
        "team": TeamPin { team: "t1".into(), founder: "a1".into(), founder_root: alex_root.device.clone() },
    });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.start();

    let first = &d.get("/api/panes")[0];
    let (closing, sid) = (first["id"].as_u64().unwrap(), first["session"].as_u64().unwrap());
    let open = d.post("/api/run", json!({ "command": "sh", "session": sid.to_string() }))["pane"].as_u64().unwrap();
    let (closing_t, open_t) = (closing.to_string(), open.to_string());
    let cy = Some(vec!["a3".to_owned()]);
    d.wait_for("cy reads both panes", || newest(&f, "pane", &closing_t) == cy && newest(&f, "pane", &open_t) == cy);
    d.post(&format!("/api/panes/{closing}/close"), json!({}));
    d.wait_for("the closed pane held", || held_panes(&f).contains(&closing_t));

    // Dee joins the team and Eve is shared the session (which nudges the
    // daemon to read the roster): the open pane is theirs, the closed one
    // isn't. Every send works out the closed pane's list again, so a wider
    // one would have gone with it.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 2, members(true, true))], certs.clone()));
    d.post(
        "/api/acl",
        json!({ "session": sid, "principal": "account:a9", "name": "eve", "role": "viewer", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    d.wait_for("dee and eve on the open pane", || {
        newest(&f, "pane", &open_t).is_some_and(|l| l.contains(&"a5".to_owned()) && l.contains(&"a9".to_owned()))
    });
    assert_eq!(newest(&f, "pane", &closing_t), cy, "nobody new reads the closed pane");

    // Cy leaves the team: the closed pane goes from him too.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 3, members(false, true))], certs.clone()));
    d.post("/api/acl", json!({ "session": sid, "principal": "account:a9", "role": null }));
    d.wait_for("cy off the closed pane", || newest(&f, "pane", &closing_t) == Some(vec![]));
    assert_eq!(newest(&f, "pane", &open_t), Some(vec!["a5".to_owned()]));

    // Back in the team, he reads the open pane again and not the closed
    // one: taken off it is for good.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 4, members(true, true))], certs));
    d.post(
        "/api/acl",
        json!({ "session": sid, "principal": "account:a9", "name": "eve", "role": "viewer", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    d.wait_for("cy back on the open pane", || {
        newest(&f, "pane", &open_t).is_some_and(|l| l.contains(&"a3".to_owned()))
    });
    assert_eq!(newest(&f, "pane", &closing_t), Some(vec![]), "cy back on the closed pane");
}

/// Alex's (`a1`) machine in team `t1`, joined to the fake control at
/// `url` and started: the roster is what `f` serves, fetched at the first
/// refresh.
fn team_machine(name: &str, f: &Fake, url: &str, alex: &DeviceKeys, alex_root: &Cert) -> arugula_testkit::Daemon {
    let mut d = arugulad!(name).no_provider().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
    d.stop();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut daemon_cert = Cert::new(&daemon, "a1", Kind::Daemon, "box");
    daemon_cert.sign_with(alex);
    *f.certs.lock().unwrap() = vec![alex_root.clone(), daemon_cert.clone()];
    let saved = json!({
        "url": url, "trust": { "account": "a1", "root": alex_root.device }, "cert": daemon_cert,
        "certs": [alex_root], "revocations": [], "login": "alex",
        "team": TeamPin { team: "t1".into(), founder: "a1".into(), founder_root: alex_root.device.clone() },
    });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.start();
    d
}

/// Team `t1`: Alex (`a1`) its owner and, if `cy`, Cy (`a3`) an editor;
/// the roster at `version` and the members' certificates.
fn alex_and_cy(alex: &DeviceKeys, alex_root: &Cert, cy_root: &Cert, version: u64, cy: bool) -> Team {
    let mut members = vec![member("a1", alex_root, TeamRole::Owner)];
    if cy {
        members.push(member("a3", cy_root, TeamRole::Editor));
    }
    let mut certs = AccountCerts::new();
    for (a, c) in [("a1", alex_root), ("a3", cy_root)] {
        certs.insert(a.into(), (vec![c.clone()], vec![]));
    }
    (vec![roster(alex, version, members)], certs)
}

/// Share the session `sid` with Eve (`a9`) as a viewer, or take it away:
/// a change in who may read what, which also has the daemon look at
/// control again.
fn eve(d: &arugula_testkit::Daemon, sid: u64, on: bool) {
    let share = if on {
        json!({ "session": sid, "principal": "account:a9", "name": "eve", "role": "viewer", "history": true,
                "root": DeviceKeys::generate().id() })
    } else {
        json!({ "session": sid, "principal": "account:a9", "role": null })
    };
    d.post("/api/acl", share);
}

/// A closed pane, and an open one in the same session, as `(closed, open,
/// session)`: the first pane closes once `ready` holds for the lists.
fn close_one(
    d: &arugula_testkit::Daemon,
    f: &Fake,
    ready: impl Fn(&Fake, &str, &str) -> bool,
) -> (String, String, u64) {
    let first = &d.get("/api/panes")[0];
    let (closing, sid) = (first["id"].as_u64().unwrap(), first["session"].as_u64().unwrap());
    let open = d.post("/api/run", json!({ "command": "sh", "session": sid.to_string() }))["pane"].as_u64().unwrap();
    let (closing_t, open_t) = (closing.to_string(), open.to_string());
    d.wait_for("the lists before the pane closes", || ready(f, &closing_t, &open_t));
    d.post(&format!("/api/panes/{closing}/close"), json!({}));
    d.wait_for("the pane closed", || !d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == closing));
    (closing_t, open_t, sid)
}

/// Taken off a closed pane's thread is for good even while chat is off:
/// a share taken away and given again before the machine next says its
/// lists doesn't bring the closed pane back.
#[test]
fn a_revoke_while_chat_is_off_sticks_to_a_closed_pane() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d =
        arugulad!("offreaders").no_provider().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    let share = |sid: u64, on: bool| {
        let body = if on {
            json!({ "session": sid, "principal": "account:a2", "name": "alex", "role": "editor", "history": true,
                    "root": DeviceKeys::generate().id() })
        } else {
            json!({ "session": sid, "principal": "account:a2", "role": null })
        };
        d.post("/api/acl", body);
    };
    let sid = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    share(sid, true);
    let (closed, open, sid) = close_one(&d, &f, |_, _, _| true);
    share(sid, false);
    share(sid, true);
    assert!(f.readers.lock().unwrap().is_empty(), "chat is off: nothing was said");

    // Chat on: the first lists said have alex on the open pane only.
    *f.chat.lock().unwrap() = true;
    d.stop();
    d.start();
    d.wait_for("the lists", || newest(&f, "pane", &open).is_some());
    assert_eq!(newest(&f, "pane", &open), Some(vec!["a2".to_owned()]));
    assert_eq!(newest(&f, "pane", &closed), Some(vec![]), "a share again brought the closed pane back");
}

/// A pane that closes before a team machine has its roster gets the
/// team's part of who read it once the roster comes: its members, and
/// not someone shared the session after it closed.
#[test]
fn a_pane_closed_before_the_roster_gets_only_the_team() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, cy_root) = device("a3");
    let d = team_machine("pendingreaders", &f, &url, &alex, &alex_root);
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    let (closed, open, sid) = close_one(&d, &f, |_, _, _| true);

    // Eve is shared the session after it closed; then the roster comes.
    eve(&d, sid, true);
    *f.team.lock().unwrap() = Some(alex_and_cy(&alex, &alex_root, &cy_root, 1, true));
    eve(&d, sid, false);
    eve(&d, sid, true);
    d.wait_for("cy and eve on the open pane", || {
        newest(&f, "pane", &open).is_some_and(|l| l.contains(&"a3".to_owned()) && l.contains(&"a9".to_owned()))
    });
    d.wait_for("cy on the closed pane", || newest(&f, "pane", &closed) == Some(vec!["a3".to_owned()]));
}

/// A team's lock keeps its members from a closed pane's thread while it
/// lasts, and not after: a lock isn't leaving the team.
#[test]
fn a_teams_lock_doesnt_take_a_closed_pane_for_good() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, cy_root) = device("a3");
    *f.team.lock().unwrap() = Some(alex_and_cy(&alex, &alex_root, &cy_root, 1, true));
    let d = team_machine("lockreaders", &f, &url, &alex, &alex_root);
    let cy = Some(vec!["a3".to_owned()]);
    let (closed, open, sid) = close_one(&d, &f, |f, c, o| {
        newest(f, "pane", c) == Some(vec!["a3".to_owned()]) && newest(f, "pane", o).is_some()
    });
    d.wait_for("the closed pane held", || held_panes(&f).contains(&closed));

    // Locked: cy is in no list.
    *f.locked.lock().unwrap() = (true, None);
    eve(&d, sid, true);
    d.wait_for("cy off both panes while locked", || {
        newest(&f, "pane", &open).is_some_and(|l| !l.contains(&"a3".to_owned()))
            && newest(&f, "pane", &closed) == Some(vec![])
    });

    // Control's plain word that it isn't locked any more doesn't lift it:
    // cy is still in no list.
    *f.locked.lock().unwrap() = (false, None);
    eve(&d, sid, false);
    d.wait_for("eve off the open pane", || newest(&f, "pane", &open).is_some_and(|l| !l.contains(&"a9".to_owned())));
    assert!(!newest(&f, "pane", &open).unwrap().contains(&"a3".to_owned()), "a plain word unlocked it");
    assert_eq!(newest(&f, "pane", &closed), Some(vec![]));

    // Its owner lifts the lock: cy reads both again.
    let mut lock = TeamLock {
        team: "t1".into(),
        locked: false,
        version: 1,
        at: arugula_e2e::now_ms(),
        by: String::new(),
        sig: String::new(),
    };
    lock.sign_with(&alex);
    *f.locked.lock().unwrap() = (false, Some(lock));
    eve(&d, sid, true);
    d.wait_for("cy back on the closed pane", || newest(&f, "pane", &closed) == cy);
    assert!(newest(&f, "pane", &open).is_some_and(|l| l.contains(&"a3".to_owned())));
}

/// The owner's signed word that team `team` (whose roster is at
/// `version`) is unlocked.
fn unlock(team: &str, version: u64, owner: &DeviceKeys) -> TeamLock {
    let mut lock = TeamLock {
        team: team.into(),
        locked: false,
        version,
        at: arugula_e2e::now_ms(),
        by: String::new(),
        sig: String::new(),
    };
    lock.sign_with(owner);
    lock
}

/// A member who leaves the team while it's locked is off a closed pane's
/// thread for good: the lock lifted, they don't read it.
#[test]
fn a_member_leaving_during_a_lock_is_gone_for_good() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, cy_root) = device("a3");
    *f.team.lock().unwrap() = Some(alex_and_cy(&alex, &alex_root, &cy_root, 1, true));
    let d = team_machine("leavelockreaders", &f, &url, &alex, &alex_root);
    let (closed, open, sid) = close_one(&d, &f, |f, c, o| {
        newest(f, "pane", c) == Some(vec!["a3".to_owned()]) && newest(f, "pane", o).is_some()
    });
    d.wait_for("the closed pane held", || held_panes(&f).contains(&closed));

    // Locked, and cy leaves the team meanwhile.
    *f.locked.lock().unwrap() = (true, None);
    eve(&d, sid, true);
    d.wait_for("cy off the panes while locked", || newest(&f, "pane", &closed) == Some(vec![]));
    let (rosters, certs) = alex_and_cy(&alex, &alex_root, &cy_root, 1, true);
    let (later, _) = alex_and_cy(&alex, &alex_root, &cy_root, 2, false);
    *f.team.lock().unwrap() = Some((rosters.into_iter().chain(later).collect(), certs));
    eve(&d, sid, false);
    d.wait_for("eve off the open pane", || newest(&f, "pane", &open).is_some_and(|l| !l.contains(&"a9".to_owned())));

    // Back in the team and unlocked: the open pane is his, the closed one
    // isn't.
    let (back, certs) = alex_and_cy(&alex, &alex_root, &cy_root, 3, true);
    let mut all = f.team.lock().unwrap().clone().unwrap().0;
    all.extend(back);
    *f.team.lock().unwrap() = Some((all, certs));
    *f.locked.lock().unwrap() = (false, Some(unlock("t1", 3, &alex)));
    eve(&d, sid, true);
    d.wait_for("cy on the open pane", || newest(&f, "pane", &open).is_some_and(|l| l.contains(&"a3".to_owned())));
    assert_eq!(newest(&f, "pane", &closed), Some(vec![]), "he left during the lock");
}

/// A team a session was shared with is locked; meanwhile its share of the
/// session is taken away and given again. Its members don't get the
/// session's closed pane back when the lock is lifted: while the share was
/// gone, their team didn't give them the session.
#[test]
fn a_locked_teams_share_taken_away_and_given_again_doesnt_give_back_a_closed_pane() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    // Team t2: Bo (b1) founds it, Cy (a3) an editor; alex's machine is
    // personal, and shares a session with t2.
    let (bo, bo_root) = device("b1");
    let (_, cy_root) = device("a3");
    let t2 = |version: u64| -> Team {
        let members = vec![member("b1", &bo_root, TeamRole::Owner), member("a3", &cy_root, TeamRole::Editor)];
        let mut certs = AccountCerts::new();
        for (a, c) in [("b1", &bo_root), ("a3", &cy_root)] {
            certs.insert(a.into(), (vec![c.clone()], vec![]));
        }
        let mut r = Roster {
            v: 1,
            team: "t2".into(),
            name: "Other".into(),
            version,
            at: version,
            members,
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(&bo);
        (vec![r], certs)
    };
    f.teams.lock().unwrap().insert("t2".into(), (t2(1), (false, None)));
    let mut d = arugulad!("sharedlockreaders")
        .no_provider()
        .no_tailscale()
        .env_remove("ARUGULA_FEATURES")
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    let share = |sid: u64, on: bool| {
        let body = if on {
            json!({ "session": sid, "principal": "team:t2", "name": "Other", "role": "editor", "history": true,
                    "root": format!("b1.{}", bo_root.device) })
        } else {
            json!({ "session": sid, "principal": "team:t2", "role": null })
        };
        d.post("/api/acl", body);
    };
    let sid = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    share(sid, true);
    // Bo, t2's owner, reads as Cy does; a lock keeps out only Cy.
    let has = |l: Option<Vec<String>>, a: &str| l.is_some_and(|l| l.iter().any(|x| x == a));
    let (closed, open, sid) =
        close_one(&d, &f, |f, c, o| has(newest(f, "pane", c), "a3") && has(newest(f, "pane", o), "a3"));
    d.wait_for("the closed pane held", || held_panes(&f).contains(&closed));

    // t2 locked: cy is in no list, Bo still is. Its share taken away and
    // given again meanwhile.
    f.teams.lock().unwrap().get_mut("t2").unwrap().1 = (true, None);
    eve(&d, sid, true);
    d.wait_for("cy off both panes while locked", || {
        newest(&f, "pane", &closed) == Some(vec!["b1".to_owned()]) && !has(newest(&f, "pane", &open), "a3")
    });
    share(sid, false);
    share(sid, true);

    // Unlocked by Bo: cy reads the open pane again, not the closed one;
    // nor does Bo, whose team's share was gone too.
    f.teams.lock().unwrap().get_mut("t2").unwrap().1 = (false, Some(unlock("t2", 1, &bo)));
    eve(&d, sid, false);
    d.wait_for("cy on the open pane", || has(newest(&f, "pane", &open), "a3"));
    assert_eq!(newest(&f, "pane", &closed), Some(vec![]), "the share given again gave back the closed pane");
}

/// While the machine's enrollment can't be read, who's in its team isn't
/// known: a pane that closes then gets the team's part of who read it once
/// the enrollment reads again.
#[test]
fn a_pane_closed_while_the_enrollment_is_unreadable_keeps_the_team() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, cy_root) = device("a3");
    *f.team.lock().unwrap() = Some(alex_and_cy(&alex, &alex_root, &cy_root, 1, true));
    let mut d = team_machine("unreadablereaders", &f, &url, &alex, &alex_root);
    let first = &d.get("/api/panes")[0];
    let (closing, sid) = (first["id"].as_u64().unwrap(), first["session"].as_u64().unwrap());
    // Another pane, so the session stays when this one closes.
    d.post("/api/run", json!({ "command": "sh", "session": sid.to_string() }));
    let closing_t = closing.to_string();
    d.wait_for("cy reads the pane", || newest(&f, "pane", &closing_t) == Some(vec!["a3".to_owned()]));

    // The enrollment unreadable over a restart; the pane closes meanwhile.
    d.stop();
    let path = d.state.join("control.json");
    let good = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{ not json").unwrap();
    d.start();
    d.post(&format!("/api/panes/{closing}/close"), json!({}));
    d.wait_for("the pane closed", || !d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == closing));
    let sent = f.readers.lock().unwrap().len();
    std::fs::write(&path, &good).unwrap();
    d.wait_for("the lists again", || f.readers.lock().unwrap().len() > sent);
    eve(&d, sid, true);
    d.wait_for("cy on the closed pane", || newest(&f, "pane", &closing_t) == Some(vec!["a3".to_owned()]));
}

/// A Cloud workspace of team `t1`'s project, whose founder Alex (`a1`)
/// signed the delegation, with Bea (`a2`) an editor and Cy (`a3`) a
/// viewer: the box admits Bea as its owner, so she reads every pane's
/// thread, a private one included, from the start; Cy reads what a viewer
/// may, watching. Demoted to a viewer, Bea watches like Cy; taken out of
/// the team, she's in no list (native #705).
///
/// A hosted refresh carries no Labs flags, so control's chat setting
/// never reaches a hosted box on its own: the test writes what control
/// would have said into the box's state dir, and the daemon reads it.
#[test]
fn a_hosted_workspaces_editor_reads_every_panes_thread_and_its_viewer_watches() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let (alex, alex_root) = device("a1");
    let (_, bea_root) = device("a2");
    let (_, cy_root) = device("a3");
    let members = |bea: Option<TeamRole>| {
        let mut m = vec![member("a1", &alex_root, TeamRole::Owner)];
        m.extend(bea.map(|r| member("a2", &bea_root, r)));
        m.push(member("a3", &cy_root, TeamRole::Viewer));
        m
    };
    let mut certs = AccountCerts::new();
    for (a, c) in [("a1", &alex_root), ("a2", &bea_root), ("a3", &cy_root)] {
        certs.insert(a.into(), (vec![c.clone()], vec![]));
    }
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 1, members(Some(TeamRole::Editor)))], certs.clone()));
    let key = HostedKey::generate();
    let mut delegation = Delegation {
        project: "prj_1".into(),
        owner: "team:t1".into(),
        key: key.public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    delegation.sign_with(&alex);
    *f.hosted.lock().unwrap() = Some(HostedProject {
        key: Arc::new(key),
        delegation,
        pin: TeamPin { team: "t1".into(), founder: "a1".into(), founder_root: alex_root.device.clone() },
        cert: None,
    });

    let state = std::env::temp_dir().join(format!("arugula-hosted-readers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();
    // The box is set up already.
    std::fs::write(state.join("hosted-setup.json"), json!({ "status": "ready", "reported": true }).to_string())
        .unwrap();
    let d = arugulad!("hostedreaders")
        .no_provider()
        .no_tailscale()
        .env_remove("ARUGULA_FEATURES")
        .state_dir(&state)
        .args(["--hosted"])
        .env("ARUGULA_HOSTED_TICKET", "ticket-1")
        .env("ARUGULA_CONTROL_URL", &url)
        .env("ARUGULA_HOSTED_RUN_USER", "")
        .env("RUST_LOG", "info")
        .args(["--log-file", &state.join("arugulad.log").to_string_lossy()])
        .wait_secs(30)
        .start();
    // On a timeout: what control was told, and the box's log.
    let wait = |what: &str, mut ok: Box<dyn FnMut() -> bool>| {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !ok() {
            if Instant::now() > deadline {
                let log = std::fs::read_to_string(state.join("arugulad.log")).unwrap_or_default();
                let tail: Vec<&str> = log.lines().rev().take(40).collect();
                let lists: Vec<String> = f.readers.lock().unwrap().iter().map(|(_, b)| b.to_string()).collect();
                panic!("timed out waiting for {what}\nlists:\n{}\nlog:\n{}", lists.join("\n"), tail.join("\n"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    // Its first refresh: the box has taken its enrollment up.
    wait("the box enrolled", Box::new(|| *f.hosted_refreshes.lock().unwrap() > 0));
    // Control has chat on for the team: written once the box is in
    // control (before, a daemon not in control forgets the file).
    std::fs::write(state.join("flags.control.json"), json!({ "account": {}, "team": { "chat": true } }).to_string())
        .unwrap();

    let pane = match d.get("/api/panes").as_array().and_then(|p| p.first().cloned()) {
        Some(p) => p,
        None => {
            let run = d.post("/api/run", json!({ "command": "sh", "session": null }));
            let id = run["pane"].as_u64().unwrap();
            d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap()
        }
    };
    let (pane, session) = (pane["id"].as_u64().unwrap(), pane["session"].as_u64().unwrap().to_string());
    let pane_t = pane.to_string();
    let sid = session.parse::<u64>().unwrap();
    // A share with Dee (`a9`) as a viewer, given or taken away: the change
    // that sends the lists, and nudges the box to look at control again.
    let dee = |on: bool| {
        let share = if on {
            json!({ "session": sid, "principal": "account:a9", "name": "dee", "role": "viewer", "history": true,
                    "root": DeviceKeys::generate().id() })
        } else {
            json!({ "session": sid, "principal": "account:a9", "role": null })
        };
        d.post("/api/acl", share);
    };
    dee(true);
    // Bea as the owner, from the start and not watching; Dee and Cy by
    // their roles (a share, then the team), watching. Never Alex, whose
    // account the box is.
    let all = json!([
        { "account": "a2", "from": null },
        { "account": "a9", "from": null, "watching": true },
        { "account": "a3", "from": null, "watching": true },
    ]);
    wait("the pane's readers", Box::new(|| lists_for(&f, "pane", &pane_t).contains(&all)));
    wait("the session's readers", Box::new(|| lists_for(&f, "session", &session).contains(&all)));

    // Private: Cy and Dee lose the pane's thread at once, Bea keeps it.
    set_private(&d, pane, true);
    wait(
        "bea alone on the private pane",
        Box::new(|| newest(&f, "pane", &pane_t).as_deref() == Some(&["a2".to_owned()])),
    );
    assert_eq!(newest(&f, "session", &session).unwrap(), ["a2", "a9", "a3"]);

    // Demoted to a viewer: once the box has the roster, Bea watches the
    // session like Cy, and the private pane is Alex's alone.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 2, members(Some(TeamRole::Viewer)))], certs.clone()));
    dee(false);
    let watching = json!([
        { "account": "a2", "from": null, "watching": true },
        { "account": "a3", "from": null, "watching": true },
    ]);
    wait("bea watching the session", Box::new(|| lists_for(&f, "session", &session).contains(&watching)));
    assert_eq!(newest(&f, "pane", &pane_t).unwrap(), Vec::<String>::new(), "a private pane: its owner's alone");

    // Taken out of the team: in no list.
    *f.team.lock().unwrap() = Some((vec![roster(&alex, 3, members(None))], certs));
    dee(true);
    wait(
        "bea gone",
        Box::new(|| newest(&f, "session", &session).as_deref() == Some(&["a9".to_owned(), "a3".to_owned()])),
    );
    assert!(since_last(&f) < Duration::from_secs(60));
    let _ = std::fs::remove_dir_all(&state);
}
