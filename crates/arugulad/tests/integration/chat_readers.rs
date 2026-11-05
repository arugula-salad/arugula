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
    team::{AccountCerts, Member, Roster, TeamPin, TeamRole},
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
    /// Any call to the import route of M124, which is gone.
    imports: Arc<Mutex<Vec<Value>>>,
    /// Each reader list posted, and when.
    readers: Arc<Mutex<Vec<(Instant, Value)>>>,
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
    Json(json!({ "locked": false, "rosters": rosters, "certs": certs }))
}

async fn import(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.imports.lock().unwrap().push(b);
    Json(json!({ "rooms": 0, "messages": 0, "already": 0 }))
}

async fn readers(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    let n = b["targets"].as_array().map_or(0, Vec::len);
    f.readers.lock().unwrap().push((Instant::now(), b));
    Json(json!({ "stored": n }))
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
        .no_wisp()
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
        arugulad!("coownerreaders").no_wisp().no_tailscale().env_remove("ARUGULA_FEATURES").wait_secs(30).start();
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
        .no_wisp()
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
