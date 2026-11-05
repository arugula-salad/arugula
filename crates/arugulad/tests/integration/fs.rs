//! M7: the `fs` methods on this host, `cd` only into an idle shell, panes
//! started in a directory, and generated names. The same through a
//! provider, on a sandbox with no daemon of ours in it, ran against a real
//! wispd and went with the Sprites adapter; the loam adapter's `fs` calls
//! are tested against a fake loam in `arugulad-labs`.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::Duration;

use arugula_testkit::arugulad;
use serde_json::{Value, json};

/// A testkit daemon, halted when dropped.
struct Daemon(arugula_testkit::Daemon);

impl std::ops::Deref for Daemon {
    type Target = arugula_testkit::Daemon;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.0.halt();
    }
}

impl Daemon {
    fn new(tag: &str) -> Self {
        Self(arugulad!(&format!("m7-{tag}")).labs().no_provider().start())
    }

    fn pane(&self, id: u64) -> Value {
        self.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap_or(Value::Null)
    }

    fn wait_for(&self, what: &str, secs: u64, f: impl FnMut() -> bool) {
        arugula_testkit::wait_for(what, Duration::from_secs(secs), f);
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A GET's response head over the socket, lowercase.
fn head_of(d: &Daemon, path: &str) -> String {
    use std::io::{BufRead, BufReader, Write};
    let mut s = std::os::unix::net::UnixStream::connect(d.sock()).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut r = BufReader::new(s);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap() == 0 || line.trim().is_empty() {
            break;
        }
        head.push_str(&line);
    }
    head.to_ascii_lowercase()
}

fn names(list: &Value) -> Vec<String> {
    list["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn this_hosts_files_cd_and_names() {
    let d = Daemon::new("local");
    // Physical path: on macOS the temp dir is under /var, a link to /private/var.
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilg-m7-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("notes.txt"), "0123456789").unwrap();
    std::os::unix::fs::symlink("/proc/self/environ", root.join("env")).unwrap();
    let r = root.display().to_string();

    // The first session's name is generated.
    let first = d.get("/api/panes")[0].clone();
    let name = first["session_name"].as_str().unwrap();
    assert!(name.split(' ').count() == 2 && name.chars().all(|c| c.is_ascii_lowercase() || c == ' '), "{name}");

    let list = d.get(&format!("/api/fs/list?path={}", enc(&r)));
    assert_eq!(names(&list), ["env", "notes.txt", "src"]);
    assert_eq!(names(&d.get(&format!("/api/fs/list?path={}&dirs=1", enc(&r)))), ["src"]);
    let (status, body) = d.raw("GET", &format!("/api/fs/read?path={}/notes.txt&offset=3&len=4", enc(&r)), None);
    assert_eq!((status, body.as_str()), (200, "3456"));
    // Its size and offset.
    let head = head_of(&d, &format!("/api/fs/read?path={}/notes.txt&offset=3&len=4", enc(&r)));
    for h in ["x-arugula-size: 10\r\n", "x-arugula-offset: 3\r\n"] {
        assert!(head.contains(h), "{head}");
    }
    assert_eq!(d.get(&format!("/api/fs/stat?path={}/notes.txt", enc(&r)))["size"], 10);
    // Refused: kernel files, a link into them, the daemon's own state.
    // (macOS has no /proc: there the link leads nowhere, a 404.)
    let mut refused = vec!["/proc/self/environ".to_owned(), d.state.join("daemon-id").display().to_string()];
    if cfg!(any(target_os = "linux", target_os = "android")) {
        refused.push(format!("{r}/env"));
    }
    for p in refused {
        let (status, body) = d.raw("GET", &format!("/api/fs/read?path={}", enc(&p)), None);
        assert_eq!(status, 403, "{p}: {body}");
    }
    assert_eq!(d.raw("GET", "/api/fs/list?path=/nonexistent-m7", None).0, 404);

    // A shell started in a directory, then cd'd elsewhere at its prompt.
    let pane = d.post("/api/run", json!({ "cwd": format!("{r}/src") }))["pane"].as_u64().unwrap();
    d.wait_for("the shell in src", 10, || d.pane(pane)["cwd"] == format!("{r}/src"));
    d.wait_for("its prompt", 10, || {
        d.raw("POST", &format!("/api/panes/{pane}/cd"), Some(json!({"path": format!("{r}/src/deep")}))).0 == 200
    });
    d.wait_for("the cd", 10, || d.pane(pane)["cwd"] == format!("{r}/src/deep"));
    // Busy: refused, and nothing typed.
    d.post(&format!("/api/panes/{pane}/send"), json!({"text": "sleep 20", "enter": true}));
    d.wait_for("the command to start", 10, || !d.pane(pane)["current"].is_null());
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/cd"), Some(json!({ "path": r })));
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("running"), "{body}");

    // Recent: where panes are and were, newest first.
    let recent = d.get("/api/fs/recent");
    assert!(recent.as_array().unwrap().iter().any(|x| *x == format!("{r}/src/deep")), "{recent}");

    std::fs::remove_dir_all(&root).unwrap();
}
