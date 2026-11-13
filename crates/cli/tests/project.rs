//! `arugula project list`, `new` and `use` against a fake platform and a
//! fake daemon, and the order `--project`, `$ARUGULA_PROJECT`, the
//! session's project, a clone's linked repo and the default go in.

// The fake daemon is on a Unix socket.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex},
};

use arugula_e2e::{Cert, DeviceKeys, Kind, Trust};
use serde_json::{Value, json};
use tungstenite::{Message, protocol::Role};

fn prj(n: u8) -> String {
    format!("prj_{n:032x}")
}

/// What the fakes were asked.
#[derive(Default)]
struct State {
    /// Control: method, path and body.
    asked: Vec<(String, String, Value)>,
    /// The daemon: the intents sent over its WebSocket.
    intents: Vec<Value>,
    /// The session the pane is in has this project.
    session_project: Option<String>,
    /// The daemon refuses intents.
    refuse: bool,
}

fn projects() -> Value {
    let personal = json!({ "kind": "personal", "account": "acc_me" });
    let acme = json!({ "kind": "team", "id": "tm_1", "name": "Acme" });
    json!({ "me": { "account": "acc_me", "login": "jake", "name": "Jake" }, "projects": [
        { "id": prj(1), "name": "Jake", "team": null, "owner": personal },
        { "id": prj(2), "name": "Acme", "owner": acme,
          "team": { "id": "tm_1", "name": "Acme", "role": "owner", "members": [] } },
        { "id": prj(3), "name": "Web", "team": null, "owner": acme },
        { "id": prj(4), "name": "Old", "team": null, "owner": personal, "archived": true },
    ] })
}

/// One HTTP request: its first line, headers (lowercase names) and body.
fn read_request(r: &mut impl BufRead) -> (String, Vec<(String, String)>, Vec<u8>) {
    let mut first = String::new();
    r.read_line(&mut first).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((h, ""));
        headers.push((k.to_ascii_lowercase(), v.trim().to_owned()));
    }
    let len = headers.iter().find(|(k, _)| k == "content-length").map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    (first, headers, body)
}

fn answer(s: &mut impl Write, status: u16, body: &Value) {
    let body = body.to_string();
    let _ = write!(
        s,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn control(st: &Mutex<State>, mut s: TcpStream) {
    let (first, _, body) = read_request(&mut BufReader::new(s.try_clone().unwrap()));
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap().to_owned(), parts.next().unwrap().to_owned());
    st.lock().unwrap().asked.push((method.clone(), target.clone(), body.clone()));
    let seg: Vec<&str> = target.split('?').next().unwrap().trim_start_matches('/').split('/').collect();
    match (method.as_str(), seg.as_slice()) {
        ("GET", ["api", "projects"]) => answer(&mut s, 200, &projects()),
        ("POST", ["api", "projects"]) => {
            let v = json!({ "id": prj(9), "name": body["name"], "team": null });
            answer(&mut s, 200, &v)
        }
        // Acme links acme/app.
        ("GET", ["api", "projects", p, "issue-home"]) => {
            let repos = if *p == prj(2) { json!(["acme/app"]) } else { json!([]) };
            let v = json!({ "project": p, "home": "native", "chosen": false, "repos": repos,
                            "native_issues": 0, "both": false });
            answer(&mut s, 200, &v)
        }
        ("GET", ["api", "projects", p, "issues"]) => {
            answer(&mut s, 200, &json!({ "project": p, "issues": [], "more": false }))
        }
        _ => answer(&mut s, 404, &json!({ "error": format!("the fake has no {method} {target}") })),
    }
}

/// The pane is %7, in session 2.
fn daemon(st: &Mutex<State>, s: UnixStream) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let (first, headers, _) = read_request(&mut r);
    let mut s = s;
    if first.starts_with("GET /api/panes") {
        let project = st.lock().unwrap().session_project.clone().map(|id| json!({ "id": id, "name": "?" }));
        let panes = json!([{ "id": 7, "session": 2, "session_name": "work", "session_project": project }]);
        return answer(&mut s, 200, &panes);
    }
    assert!(first.starts_with("GET /ws"), "the fake daemon has no {first}");
    let key = headers.iter().find(|(k, _)| k == "sec-websocket-key").unwrap().1.clone();
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        s,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .unwrap();
    let mut ws = tungstenite::WebSocket::from_raw_socket(s, Role::Server, None);
    while let Ok(m) = ws.read() {
        let Message::Text(t) = m else { continue };
        let v: Value = serde_json::from_str(&t).unwrap();
        match v["type"].as_str() {
            Some("intent") => {
                let mut st = st.lock().unwrap();
                st.intents.push(v["intent"].clone());
                if st.refuse {
                    let e = json!({ "type": "error", "id": v["id"], "message": "no such session" });
                    ws.send(Message::Text(e.to_string().into())).unwrap();
                }
            }
            Some("ping") => {
                ws.send(Message::Text(json!({ "type": "pong", "id": v["id"] }).to_string().into())).unwrap();
            }
            _ => {}
        }
    }
}

struct Rig {
    dir: PathBuf,
    state: Arc<Mutex<State>>,
}

impl Rig {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("arugula-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("arugula");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let state = Arc::new(Mutex::new(State::default()));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let st = state.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                control(&st, s);
            }
        });
        let sock = UnixListener::bind(dir.join("daemon.sock")).unwrap();
        let st = state.clone();
        std::thread::spawn(move || {
            for s in sock.incoming().flatten() {
                let st = st.clone();
                std::thread::spawn(move || daemon(&st, s));
            }
        });

        // This CLI as `arugula login` leaves it: a key, and what it pinned.
        let keys = DeviceKeys::generate();
        keys.save(&config.join("cli-key")).unwrap();
        let cert = Cert {
            v: 1,
            account: "acc_me".into(),
            device: keys.id(),
            kind: Kind::Cli,
            name: "test".into(),
            noise: "00".repeat(32),
            sign: "00".repeat(32),
            created: 0,
            approver: keys.id(),
            sig: String::new(),
        };
        let trust = Trust { account: "acc_me".into(), root: keys.id() };
        let saved = json!({ "url": url, "cert": cert, "trust": trust });
        std::fs::write(config.join("cli-control.json"), saved.to_string()).unwrap();
        Rig { dir, state }
    }

    /// In `cwd` (under the rig), with `env` set; in pane %7 when `pane`.
    fn run(&self, cwd: &str, pane: bool, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_arugula"));
        c.arg("--socket")
            .arg(self.dir.join("daemon.sock"))
            .args(args)
            .current_dir(self.dir.join(cwd))
            .env("GIT_CEILING_DIRECTORIES", &self.dir)
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("HOME", &self.dir)
            .env("ARUGULA_TELEMETRY", "0")
            .env("DO_NOT_TRACK", "1")
            .env_remove("ARUGULA_PROJECT")
            .env_remove("ARUGULA_SOCK")
            .env_remove("ARUGULA_PANE")
            .env_remove("CLAUDECODE")
            .env_remove("AI_AGENT");
        if pane {
            c.env("ARUGULA_PANE", "7");
        }
        c.envs(env.iter().copied());
        c.output().unwrap()
    }

    fn ok(&self, pane: bool, args: &[&str]) -> String {
        let out = self.run("work", pane, &[], args);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{args:?}: {o}\n{e}");
        o.into_owned()
    }

    /// The project `issue list` listed, run as `run` would.
    fn listed_in(&self, cwd: &str, pane: bool, env: &[(&str, &str)], args: &[&str]) -> String {
        self.state.lock().unwrap().asked.clear();
        let mut argv = vec!["issue", "list", "--native"];
        argv.extend_from_slice(args);
        let out = self.run(cwd, pane, env, &argv);
        assert!(out.status.success(), "{argv:?}: {}", String::from_utf8_lossy(&out.stderr));
        let asked = &self.state.lock().unwrap().asked;
        let (_, path, _) = asked.iter().rev().find(|(_, p, _)| p.contains("/issues")).expect("it listed");
        path.trim_start_matches("/api/projects/").split('/').next().unwrap().to_owned()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
}

#[test]
fn list_by_owner() {
    let rig = Rig::new("list");
    rig.state.lock().unwrap().session_project = Some(prj(3));
    let out = rig.ok(true, &["project", "list"]);
    let want = format!("Personal\n  {}  Jake\nAcme\n  {}  Acme\n* {}  Web\n", prj(1), prj(2), prj(3));
    assert_eq!(out, want, "the session's project is current; the archived one is hidden");
    let out = rig.ok(false, &["project", "list", "--archived"]);
    assert!(out.contains(&format!("* {}  Jake\n  {}  Old  (archived)\n", prj(1), prj(4))), "{out}");
    let v: Value = serde_json::from_str(&rig.ok(false, &["--json", "project", "list"])).unwrap();
    assert_eq!(v["current"], prj(1));
    assert_eq!(v["owners"][1]["id"], "tm_1");
}

#[test]
fn new_personal_or_a_team_s() {
    let rig = Rig::new("new");
    assert_eq!(rig.ok(false, &["project", "new", "Notes"]), format!("{} Notes\n", prj(9)));
    rig.ok(false, &["project", "new", "Site", "--team", "acme"]);
    rig.ok(false, &["project", "new", "Ops", "--team", "tm_1"]);
    let made: Vec<Value> =
        rig.state.lock().unwrap().asked.iter().filter(|(m, ..)| m == "POST").map(|(.., b)| b.clone()).collect();
    assert_eq!(made[0], json!({ "name": "Notes", "owner": { "kind": "personal" } }));
    assert_eq!(made[1], json!({ "name": "Site", "owner": { "kind": "team", "id": "tm_1" } }));
    assert_eq!(made[2]["owner"]["id"], "tm_1");
    let out = rig.run("work", false, &[], &["project", "new", "X", "--team", "Nope"]);
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("no team called Nope"));
}

#[test]
fn use_sets_the_session_s_project() {
    let rig = Rig::new("use");
    let out = rig.ok(true, &["project", "use", "web"]);
    assert_eq!(out, format!("Session work is in project Web ({}).\n", prj(3)));
    rig.ok(true, &["project", "use", "personal"]);
    let intents = rig.state.lock().unwrap().intents.clone();
    assert_eq!(
        intents[0],
        json!({ "op": "set_session_project", "session": 2, "project": { "id": prj(3), "name": "Web" } })
    );
    assert_eq!(intents[1]["project"]["id"], prj(1));

    // Outside a pane there's no session; an unknown project and a refusal fail.
    let fails = |pane: bool, what: &str| {
        let out = rig.run("work", pane, &[], &["project", "use", what]);
        assert!(!out.status.success());
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    assert!(fails(false, "web").contains("Arugula pane"));
    assert!(fails(true, "nope").contains("no project called nope"));
    rig.state.lock().unwrap().refuse = true;
    assert!(fails(true, "web").contains("no such session"));
}

/// Asked (`--project` over `$ARUGULA_PROJECT`), then the session's, then
/// the clone's linked repo, then the default.
#[test]
fn the_current_project_s_order() {
    let rig = Rig::new("order");
    let clone = rig.dir.join("app");
    std::fs::create_dir_all(&clone).unwrap();
    git(&clone, &["init", "-q"]);
    git(&clone, &["remote", "add", "origin", "git@github.com:acme/app.git"]);
    let env = [("ARUGULA_PROJECT", "web")];

    assert_eq!(rig.listed_in("work", false, &[], &[]), prj(1), "the default");
    assert_eq!(rig.listed_in("app", false, &[], &[]), prj(2), "the linked repo's");
    rig.state.lock().unwrap().session_project = Some(prj(1));
    assert_eq!(rig.listed_in("app", true, &[], &[]), prj(1), "the session's, over the repo's");
    assert_eq!(rig.listed_in("app", true, &env, &[]), prj(3), "$ARUGULA_PROJECT, over the session's");
    assert_eq!(rig.listed_in("app", true, &env, &["--project", "acme"]), prj(2), "--project, over the env");
    // A session's project that isn't yours any more is skipped.
    rig.state.lock().unwrap().session_project = Some(prj(8));
    assert_eq!(rig.listed_in("app", true, &[], &[]), prj(2));
}
