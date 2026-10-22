//! M199: `arugula issue` against a fake platform, with only the CLI
//! signed in and no daemon: new, list, show, comment, close, reopen, edit
//! and export, where an issue goes by the project's issue home, and what
//! an agent may not do through it.

// The fake daemon socket path is a Unix one; the platform half is the same
// on Windows.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex},
};

use arugula_e2e::{Cert, DeviceKeys, Kind, Trust};
use serde_json::{Value, json};

const PROJECT: &str = "prj_00000000000000000000000000000001";

/// What the fake platform holds, and what it was asked.
#[derive(Default)]
struct State {
    issues: Vec<Value>,
    events: Vec<(String, Value)>,
    /// Comments by issue id.
    comments: Vec<(String, Value)>,
    asked: Vec<(String, String, Value)>,
    chat_off: bool,
    home: String,
    /// The project's linked repos.
    repos: Vec<String>,
    /// Control is failing: 500 for everything.
    down: bool,
}

fn issue_id(n: usize) -> String {
    format!("iss_{n:032x}")
}

fn answer(s: &mut TcpStream, status: u16, kind: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn serve(st: &Mutex<State>, mut s: TcpStream) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut first = String::new();
    r.read_line(&mut first).unwrap();
    let (mut len, mut signed) = (0, false);
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((h, ""));
        match k.to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap(),
            "x-arugula-auth" => signed = true,
            _ => {}
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap().to_owned(), parts.next().unwrap().to_owned());
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let q = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(|v| v.replace("%2A", "*").replace('+', " "))
    };
    let mut st = st.lock().unwrap();
    st.asked.push((method.clone(), target.clone(), body.clone()));
    assert!(signed, "every call is signed: {method} {target}");
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let json = |s: &mut TcpStream, status: u16, v: Value| answer(s, status, "application/json", &v.to_string());
    let now = 1_700_000_000_000u64;
    if st.down {
        return json(&mut s, 500, json!({ "error": "something went wrong" }));
    }
    let chat_off = st.chat_off;
    if chat_off && (seg.get(1) == Some(&"projects") && seg.len() > 3 || seg.get(1) == Some(&"issues")) {
        return json(&mut s, 403, json!({ "error": "native issues come with chat", "reason": "chat_off" }));
    }
    match (method.as_str(), seg.as_slice()) {
        // A linked repo's GitHub issues (step 4's M169): one, on the repo asked.
        ("POST", ["api", "forge", "ops"]) if body["op"] == "item.list" => {
            let item = json!({ "id": "fi_7", "repo": body["repo"].as_str().unwrap_or("acme/app"), "number": 7,
                "kind": "issue", "state": "open", "title": "From GitHub", "author": "octo" });
            json(&mut s, 200, json!({ "op": "item.list", "v": 1, "result": { "items": [item], "withheld": [] } }))
        }
        ("GET", ["api", "projects"]) => json(
            &mut s,
            200,
            json!({ "me": { "account": "acc_me", "login": "jake", "name": "Jake" },
                    "projects": [{ "id": PROJECT, "name": "Jake", "team": null }] }),
        ),
        ("GET", ["api", "projects", _, "issue-home"]) => {
            let v = json!({ "project": PROJECT, "home": st.home, "chosen": false,
                "repos": st.repos,
                "native_issues": st.issues.len(), "both": false });
            json(&mut s, 200, v)
        }
        ("POST", ["api", "projects", "personal" | PROJECT, "issues"]) => {
            let n = st.issues.len() + 1;
            let i = json!({ "id": issue_id(n), "project": PROJECT, "number": n, "title": body["title"],
                "body": body["body"], "state": "open", "close_reason": null, "author": "acc_me", "agent": null,
                "labels": body["labels"], "assignees": body["assignees"], "github": null,
                "created_at": now, "updated_at": now, "closed_at": null });
            st.issues.push(i.clone());
            st.events.push((
                issue_id(n),
                json!({ "id": 1, "kind": "opened", "actor": "acc_me", "agent": null,
                "at": now, "detail": {} }),
            ));
            json(&mut s, 200, i)
        }
        ("GET", ["api", "projects", _, "issues"]) => {
            let want = q("state").unwrap_or_else(|| "open".into());
            let words = q("q");
            let issues: Vec<Value> = st
                .issues
                .iter()
                .rev()
                .filter(|i| want == "all" || i["state"] == want.as_str())
                .filter(|i| {
                    words.as_ref().is_none_or(|w| i["title"].as_str().unwrap().contains(w.trim_end_matches('*')))
                })
                .cloned()
                .collect();
            json(&mut s, 200, json!({ "project": PROJECT, "issues": issues, "more": false }))
        }
        ("GET", ["api", "projects", _, "issues", "export"]) => {
            if q("format").as_deref() == Some("md") {
                let mut md = String::from("# Jake's issues\n");
                for i in &st.issues {
                    md.push_str(&format!("\n## #{} {}\n", i["number"], i["title"].as_str().unwrap()));
                }
                return answer(&mut s, 200, "text/markdown; charset=utf-8", &md);
            }
            let issues: Vec<Value> = st
                .issues
                .iter()
                .map(|i| {
                    let id = i["id"].as_str().unwrap();
                    let of = |v: &[(String, Value)]| {
                        v.iter().filter(|(k, _)| k == id).map(|(_, e)| e.clone()).collect::<Vec<_>>()
                    };
                    json!({ "issue": i, "events": of(&st.events), "comments": of(&st.comments) })
                })
                .collect();
            json(
                &mut s,
                200,
                json!({ "project": PROJECT, "project_name": "Jake", "exported_at": now,
                "issues": issues, "people": [{ "account": "acc_me", "name": "Jake" }] }),
            )
        }
        ("GET", ["api", "projects", _, "issues", n]) => {
            match st.issues.iter().find(|i| n.parse::<u64>().is_ok_and(|n| i["number"] == n)) {
                Some(i) => json(&mut s, 200, i.clone()),
                None => json(&mut s, 404, json!({ "error": "no such issue" })),
            }
        }
        ("GET", ["api", "issues", id]) => match st.issues.iter().find(|i| i["id"] == *id) {
            Some(i) => json(&mut s, 200, i.clone()),
            None => json(&mut s, 404, json!({ "error": "no such issue" })),
        },
        ("GET", ["api", "issues", id, "events"]) => {
            let events: Vec<Value> = st.events.iter().filter(|(k, _)| k == id).map(|(_, e)| e.clone()).collect();
            json(&mut s, 200, json!({ "issue": id, "events": events }))
        }
        ("PATCH", ["api", "issues", id]) => {
            let id = (*id).to_owned();
            let Some(i) = st.issues.iter_mut().find(|i| i["id"] == id.as_str()) else {
                return json(&mut s, 404, json!({ "error": "no such issue" }));
            };
            let mut events = Vec::new();
            for k in ["title", "body"] {
                if body[k].is_string() {
                    i[k] = body[k].clone();
                    events.push(json!({ "kind": "edited", "detail": { k: true } }));
                }
            }
            match body["state"].as_str() {
                Some("closed") => {
                    i["state"] = json!("closed");
                    i["close_reason"] = json!(body["reason"].as_str().unwrap_or("completed"));
                    events.push(json!({ "kind": "closed", "detail": { "reason": i["close_reason"] } }));
                }
                Some("open") => {
                    i["state"] = json!("open");
                    i["close_reason"] = Value::Null;
                    events.push(json!({ "kind": "reopened", "detail": {} }));
                }
                _ => {}
            }
            let edit = &body["labels"];
            let mut labels: Vec<Value> = i["labels"].as_array().cloned().unwrap_or_default();
            if let Some(set) = edit["set"].as_array() {
                labels = set.clone();
            }
            labels.extend(edit["add"].as_array().cloned().unwrap_or_default());
            labels.retain(|l| !edit["remove"].as_array().is_some_and(|r| r.contains(l)));
            i["labels"] = json!(labels);
            let i = i.clone();
            let events: Vec<Value> = events
                .into_iter()
                .enumerate()
                .map(|(n, mut e)| {
                    e["id"] = json!(10 + n);
                    e["actor"] = json!("acc_me");
                    e["agent"] = Value::Null;
                    e["at"] = json!(now);
                    e
                })
                .collect();
            for e in &events {
                st.events.push((id.clone(), e.clone()));
            }
            json(&mut s, 200, json!({ "issue": i, "events": events }))
        }
        ("POST", ["api", "chat", "targets"]) => {
            assert_eq!(body["kind"], "object");
            let target = body["target"].as_str().unwrap();
            let last = st.comments.iter().filter(|(k, _)| k == target).count();
            json(
                &mut s,
                200,
                json!({ "id": format!("th_{target}"), "kind": "object", "team": null, "name": "issue",
                "machine": null, "machine_name": null, "target": target, "project": PROJECT, "with": null,
                "last": last, "read": last, "unread": 0, "mentions": 0, "muted": false, "archived": false,
                "writable": true }),
            )
        }
        (m, ["api", "chat", "rooms", room, "messages"]) => {
            let issue = room.trim_start_matches("th_").to_owned();
            if m == "POST" {
                let seq = st.comments.iter().filter(|(k, _)| *k == issue).count() + 1;
                st.comments.push((
                    issue,
                    json!({ "seq": seq, "at": now, "author": "acc_me", "name": "Jake", "text": body["text"],
                    "quote": null, "pane": null, "mentions": [], "edited": null, "deleted": null,
                    "imported": null, "agent": null }),
                ));
                return json(&mut s, 200, json!({ "seq": seq, "mentions": [], "unreached": [], "pushed": [] }));
            }
            let messages: Vec<Value> =
                st.comments.iter().filter(|(k, _)| *k == issue).map(|(_, c)| c.clone()).collect();
            json(
                &mut s,
                200,
                json!({ "room": { "id": room, "kind": "object", "team": null, "name": "issue", "machine": null,
                "machine_name": null, "target": issue, "project": PROJECT, "with": null, "last": 0, "read": 0,
                "unread": 0, "mentions": 0, "muted": false, "archived": false, "writable": true },
                "messages": messages, "more": false }),
            )
        }
        _ => json(&mut s, 404, json!({ "error": format!("the fake has no {method} {path}") })),
    }
}

struct Rig {
    dir: PathBuf,
    state: Arc<Mutex<State>>,
    /// Where commands run: `work`, which isn't a git clone.
    cwd: Mutex<PathBuf>,
}

impl Rig {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("arugula-issue-native-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("arugula");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(dir.join("work")).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State { home: "native".into(), ..Default::default() }));
        let st = state.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                serve(&st, s);
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
        let cwd = Mutex::new(dir.join("work"));
        Rig { dir, state, cwd }
    }

    fn run_as(&self, agent: bool, args: &[&str]) -> Output {
        self.run_with(agent, &[], args)
    }

    fn run_with(&self, agent: bool, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_arugula"));
        c.arg("--socket")
            .arg(self.dir.join("no-daemon.sock"))
            .args(args)
            .current_dir(&*self.cwd.lock().unwrap())
            // A clone around the test's own directory isn't its.
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
        if agent {
            c.env("CLAUDECODE", "1");
        }
        c.envs(env.iter().copied());
        c.output().unwrap()
    }

    /// Its stdout; it must succeed.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run_as(false, args);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{args:?}: {o}\n{e}");
        o.into_owned()
    }

    /// Its stderr; it must fail.
    fn fails(&self, agent: bool, args: &[&str]) -> String {
        let out = self.run_as(agent, args);
        assert!(!out.status.success(), "{args:?} should fail: {}", String::from_utf8_lossy(&out.stdout));
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn patches(&self) -> Vec<Value> {
        self.state.lock().unwrap().asked.iter().filter(|(m, ..)| m == "PATCH").map(|(.., b)| b.clone()).collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn exists(p: &Path) -> bool {
    p.exists()
}

#[test]
fn the_loop_with_no_daemon() {
    let rig = Rig::new("loop");
    let first = issue_id(1);

    let out = rig.run_as(false, &["issue", "new", "-t", "The build is red", "-b", "Since Tuesday.", "--label", "bug"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("#1 {first} The build is red"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("in project Jake"), "a write names its project");
    rig.ok(&["issue", "new", "--title", "Faster export", "--assignee", "me"]);
    assert_eq!(rig.state.lock().unwrap().issues[1]["assignees"], json!(["acc_me"]), "me is the account");

    let out = rig.ok(&["issue", "list"]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("#2") && lines[0].contains("Faster export"), "{out}");
    assert!(lines[1].starts_with("#1") && lines[1].contains("[bug]") && lines[1].contains("(Jake, "), "{out}");
    assert!(rig.ok(&["issue", "list", "-q", "build"]).contains("#1"));

    assert_eq!(rig.ok(&["issue", "comment", "#1", "Seen it on main too."]).trim(), "#1: comment 1");
    assert_eq!(rig.ok(&["issue", "comment", &first, "And on the branch."]).trim(), "#1: comment 2");

    let out = rig.ok(&["issue", "show", "1"]);
    assert!(out.starts_with("#1 The build is red\nopen · opened by Jake"), "{out}");
    assert!(out.contains("Since Tuesday.") && out.contains("labels: bug"), "{out}");
    assert!(out.contains("  Seen it on main too.") && out.contains("  And on the branch."), "{out}");
    // Bare `arugula issue #N` is `show`, too.
    assert_eq!(rig.ok(&["issue", "#1"]), out);
    let v: Value = serde_json::from_str(&rig.ok(&["--json", "issue", "show", "#1"])).unwrap();
    assert_eq!(v["comments"].as_array().unwrap().len(), 2);

    let out = rig.ok(&["issue", "close", "#1", "--reason", "not-planned", "-c", "Flaky runner."]);
    assert!(out.starts_with("#1") && out.contains("closed"), "{out}");
    assert_eq!(
        rig.patches().last().unwrap(),
        &json!({ "title": null, "body": null, "state": "closed",
        "reason": "not_planned", "labels": null, "assignees": null })
    );
    assert!(rig.ok(&["issue", "show", "#1"]).contains("closed (not planned)"));
    assert!(rig.ok(&["issue", "show", "#1"]).contains("Jake closed it (not planned)"));
    assert!(!rig.ok(&["issue", "list"]).contains("#1 "), "closed ones aren't listed");
    assert!(rig.ok(&["issue", "list", "--state", "all"]).contains("#1 "));

    rig.ok(&["issue", "reopen", "1"]);
    assert_eq!(rig.state.lock().unwrap().issues[0]["state"], "open");

    // An add and a remove are two patches.
    rig.ok(&["issue", "edit", "#2", "--title", "Faster exports", "--add-label", "perf", "--remove-label", "bug"]);
    let p = rig.patches();
    assert_eq!(p[p.len() - 2]["title"], "Faster exports");
    assert_eq!(p[p.len() - 2]["labels"], json!({ "add": ["perf"] }));
    assert_eq!(p[p.len() - 1]["labels"], json!({ "remove": ["bug"] }));

    let md = rig.dir.join("work").join("issues.md");
    std::fs::write(&md, "mine").unwrap();
    assert!(rig.fails(false, &["issue", "export", "--format", "md", "-o", md.to_str().unwrap()]).contains("--force"));
    assert_eq!(std::fs::read_to_string(&md).unwrap(), "mine", "left as it was");
    rig.ok(&["issue", "export", "--format", "md", "-o", md.to_str().unwrap(), "--force"]);
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.contains("## #1 The build is red") && text.contains("## #2 Faster exports"), "{text}");
    let others: Vec<_> = std::fs::read_dir(rig.dir.join("work")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(others.len(), 1, "no temp file left: {others:?}");
    let x: Value = serde_json::from_str(&rig.ok(&["issue", "export"])).unwrap();
    assert_eq!(x["issues"].as_array().unwrap().len(), 2);
    assert_eq!(x["issues"][0]["comments"].as_array().unwrap().len(), 3, "the close's comment too");

    // No daemon was asked for any of it: its socket was never made.
    assert!(!exists(&rig.dir.join("no-daemon.sock")));
}

#[test]
fn an_agent_reads_but_doesnt_write() {
    let rig = Rig::new("agent");
    rig.ok(&["issue", "new", "-t", "One"]);
    for args in [
        &["issue", "new", "-t", "Two"][..],
        &["issue", "comment", "#1", "hi"],
        &["issue", "close", "#1"],
        &["issue", "edit", "#1", "--title", "Uno"],
    ] {
        assert!(rig.fails(true, args).contains("agent doesn't write native issues"), "{args:?}");
    }
    let out = rig.run_as(true, &["issue", "list"]);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("#1"));
    assert_eq!(rig.state.lock().unwrap().issues.len(), 1);
}

#[test]
fn the_issue_home_decides() {
    let rig = Rig::new("home");
    // A GitHub home: list lists GitHub's from control (M169); close and
    // export say where they are; --native reads the project's own.
    rig.state.lock().unwrap().home = "github".into();
    assert!(rig.ok(&["issue", "list"]).contains("From GitHub"));
    assert!(rig.fails(false, &["issue", "close", "#1"]).contains("GitHub's"));
    // A repo named, or a GitHub filter, is GitHub's whatever the home.
    rig.state.lock().unwrap().home = "native".into();
    for args in [&["issue", "list", "acme/site"][..], &["issue", "list", "--review-requested", "octo"]] {
        assert!(rig.ok(args).contains("From GitHub"), "{args:?}");
    }
    let asked = rig.state.lock().unwrap().asked.clone();
    let lists: Vec<&Value> = asked.iter().filter(|(_, t, _)| t == "/api/forge/ops").map(|(.., b)| b).collect();
    assert_eq!(
        lists.iter().map(|b| (b["repo"].as_str(), b["review_requested"].as_str())).collect::<Vec<_>>(),
        [(None, None), (Some("acme/site"), None), (None, Some("octo"))]
    );
    rig.state.lock().unwrap().home = "github".into();
    assert!(rig.fails(false, &["issue", "comment", "#3", "x"]).contains("on GitHub"));
    // `N` on a GitHub home is the block %N, as it was: the daemon's.
    assert!(rig.fails(false, &["issue", "comment", "3", "x"]).contains("can't reach arugulad"));
    assert!(rig.ok(&["issue", "list", "--native"]).contains("No issues."));
    rig.ok(&["issue", "new", "-t", "Native anyway", "--native"]);
    assert!(rig.ok(&["issue", "show", "#1", "--native"]).contains("Native anyway"));

    // Chat off: what only the platform has says so; the rest is GitHub's,
    // as before.
    rig.state.lock().unwrap().chat_off = true;
    assert!(rig.fails(false, &["issue", "list", "--native"]).contains("Labs"));
    assert!(rig.fails(false, &["issue", "export", "--project", "personal"]).contains("Labs"));
    assert!(rig.fails(false, &["issue", "list"]).contains("Labs"));
    assert!(rig.fails(false, &["issue", "new", "-t", "x"]).contains("can't reach arugulad"));
}

/// In a GitHub clone, the current project is the one linking its repo; a
/// repo no project links keeps GitHub's issues (its blocks, the daemon's).
#[test]
fn the_directory_names_the_project() {
    let rig = Rig::new("clone");
    let clone = |name: &str, remote: &str| {
        let d = rig.dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let git =
            |args: &[&str]| assert!(Command::new("git").args(args).current_dir(&d).output().unwrap().status.success());
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", remote]);
        d
    };
    *rig.cwd.lock().unwrap() = clone("other", "git@github.com:acme/other.git");
    assert!(rig.fails(false, &["issue", "new", "-t", "x"]).contains("can't reach arugulad"));
    assert!(rig.state.lock().unwrap().issues.is_empty());
    // The project's, with --project.
    rig.ok(&["issue", "new", "-t", "Here anyway", "--project", "personal"]);

    *rig.cwd.lock().unwrap() = clone("app", "https://github.com/acme/app.git");
    rig.state.lock().unwrap().repos = vec!["acme/app".into()];
    assert!(rig.ok(&["issue", "list"]).contains("Here anyway"), "native: the linked project's home");
    rig.state.lock().unwrap().home = "github".into();
    assert!(rig.ok(&["issue", "list"]).contains("acme/app#7"), "GitHub's, of the linked repo, from control");
    let asked = rig.state.lock().unwrap().asked.clone();
    let (.., list) = asked.iter().rfind(|(_, t, _)| t == "/api/forge/ops").unwrap();
    assert_eq!(list["repo"], "acme/app");
}

/// A clone whose remotes aren't github.com repos can't name its project:
/// it's asked for, not guessed. Credentials in a remote are never shown.
#[test]
fn an_unmapped_clone_asks_for_the_project() {
    let rig = Rig::new("unmapped");
    let clone = |name: &str, remote: &str| {
        let d = rig.dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let git =
            |args: &[&str]| assert!(Command::new("git").args(args).current_dir(&d).output().unwrap().status.success());
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", remote]);
        d
    };
    *rig.cwd.lock().unwrap() = clone("alias", "git@github-work:acme/app.git");
    for args in [&["issue", "list"][..], &["issue", "new", "-t", "x"], &["issue", "comment", "5", "private"]] {
        let err = rig.fails(false, args);
        assert!(err.contains("--project") && !err.contains("can't reach arugulad"), "{args:?}: {err}");
    }
    rig.ok(&["issue", "new", "-t", "Named", "--project", "personal"]);

    *rig.cwd.lock().unwrap() = clone("ghe", "https://x-access-token:s3cret@ghe.example.com/acme/app.git");
    let err = rig.fails(false, &["issue", "list"]);
    assert!(err.contains("ghe.example.com/acme/app.git") && !err.contains("s3cret"), "{err}");

    // A token in a github.com remote is left out, and the repo matched.
    *rig.cwd.lock().unwrap() = clone("token", "https://x-access-token:s3cret@github.com/acme/app.git");
    rig.state.lock().unwrap().repos = vec!["acme/app".into()];
    assert!(rig.ok(&["issue", "list"]).contains("Named"));

    // $ARUGULA_PROJECT says so.
    *rig.cwd.lock().unwrap() = rig.dir.join("work");
    let out = rig.run_with(false, &[("ARUGULA_PROJECT", "personal")], &["issue", "list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("project Jake, from $ARUGULA_PROJECT"));
}

/// With control failing, nothing is sent to GitHub in its place: `N`
/// there would be the block %N, which may be public.
#[test]
fn control_down_is_an_error_not_github() {
    let rig = Rig::new("down");
    rig.state.lock().unwrap().down = true;
    for args in [
        &["issue", "comment", "5", "private"][..],
        &["issue", "new", "-t", "private"],
        &["issue", "5"],
        &["issue", "list"],
    ] {
        let err = rig.fails(false, args);
        assert!(err.contains("asking Arugula control") && !err.contains("can't reach arugulad"), "{args:?}: {err}");
    }
    // Asked for GitHub's by name, it's GitHub's.
    assert!(rig.fails(false, &["issue", "comment", "%5", "x"]).contains("can't reach arugulad"));
}
