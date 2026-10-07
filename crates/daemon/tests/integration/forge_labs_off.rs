//! Forgejo and GitLab are Labs (#457): where Labs is off on the machine
//! (no `labs` file in the state dir) or in the build (`--no-default-features`)
//! only GitHub's pull requests and issues open. A link, a bare
//! `OWNER/REPO#N` and a whole config that come out as Forgejo's or GitLab's
//! are refused with a message; a layout saved with such a block brings it
//! back, unavailable; and the two webhook routes answer as unknown ones do.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use serde_json::{Value, json};

use crate::agentd::Daemon;

/// A daemon with no `gh`, `tea` or `glab` to find, and no `labs` file.
fn daemon() -> Daemon {
    Daemon::child_env(&["--wisp-token-file", "/nonexistent"], &[("PATH", "/usr/bin:/bin")])
}

fn open(d: &Daemon, config: Value) -> (u16, String) {
    d.raw("POST", "/api/blocks", Some(json!({ "type": "forge", "config": config, "local": true })))
}

/// What a refusal says: this build has Labs and the machine has it off, or
/// this build lacks it.
fn refusal(name: &str) -> String {
    if cfg!(feature = "labs") {
        format!("{name} blocks are in Labs (turn Labs on to open them)")
    } else {
        format!("A {name} block isn't in this build (built without labs)")
    }
}

fn block_configs(layout: &mut Value, f: &mut dyn FnMut(&mut Value)) {
    match layout {
        Value::Object(o) => {
            if o.contains_key("repo") && o.contains_key("number") && o.contains_key("provider") {
                f(layout);
                return;
            }
            for v in o.values_mut() {
                block_configs(v, f);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|v| block_configs(v, f)),
        _ => {}
    }
}

#[test]
fn a_daemon_without_labs_opens_github_and_refuses_forgejo_and_gitlab() {
    let d = daemon();
    for config in [
        // Forgejo's link, and what says it is Forgejo's some other way.
        json!({ "pr": "https://git.example.test/o/r/pulls/3" }),
        json!({ "issue": "https://git.example.test/o/r/issues/3" }),
        json!({ "pr": "o/r#3", "provider": "forgejo" }),
        json!({ "pr": "o/r#3", "host": "git.example.test" }),
        json!({ "repo": "o/r", "number": 3, "provider": "forgejo" }),
    ] {
        let (status, body) = open(&d, config.clone());
        assert_eq!(status, 400, "{config}: {body}");
        assert!(body.contains(&refusal("Forgejo")), "{config}: {body}");
    }
    for config in [
        json!({ "pr": "https://gitlab.com/g/p/-/merge_requests/2" }),
        json!({ "pr": "g/p!2" }),
        json!({ "pr": "o/r#3", "host": "gitlab.example.org" }),
    ] {
        let (status, body) = open(&d, config.clone());
        assert_eq!(status, 400, "{config}: {body}");
        assert!(body.contains(&refusal("GitLab")), "{config}: {body}");
    }
    assert!(d.get("/api/panes").as_array().unwrap().iter().all(|p| p["type"] != "forge"));

    // GitHub's open as they always did (nothing here can read them).
    for config in [
        json!({ "pr": "https://github.com/cli/cli/pull/14519" }),
        json!({ "issue": "https://github.com/cli/cli/issues/9" }),
        json!({ "provider": "github", "repo": "cli/cli", "number": 3 }),
        // A bare reference is GitHub's where Labs is off.
        json!({ "pr": "cli/cli#3" }),
        json!({ "issue": "new", "title": "T", "repo": "o/r" }),
    ] {
        let (status, body) = open(&d, config.clone());
        assert_eq!(status, 200, "{config}: {body}");
    }
}

#[test]
fn a_saved_forgejo_block_comes_back_unavailable_and_stays_in_the_layout() {
    let mut d = daemon();
    let (status, body) = open(&d, json!({ "provider": "github", "repo": "o/r", "number": 3 }));
    assert_eq!(status, 200, "{body}");
    let block = serde_json::from_str::<Value>(&body).unwrap()["block"].as_u64().unwrap();
    d.stop();
    // A daemon with Labs on saved it as Forgejo's.
    let path = d.state.join("layout.json");
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut n = 0;
    block_configs(&mut saved, &mut |c| {
        c["provider"] = json!("forgejo");
        n += 1;
    });
    assert_eq!(n, 1, "the block's config is in the layout: {saved}");
    std::fs::write(&path, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();

    d.start();
    let st = d.state(block);
    let why = refusal("Forgejo");
    assert_eq!(st["error"].as_str(), Some(why.as_str()), "{st}");
    assert_eq!(
        (st["provider"].as_str(), st["repo"].as_str(), st["loading"].as_bool()),
        (Some("forgejo"), Some("o/r"), Some(false))
    );
    // It keeps reading as unavailable, whatever the poll does.
    std::thread::sleep(std::time::Duration::from_millis(700));
    assert_eq!(d.state(block)["error"].as_str(), Some(why.as_str()));

    // And saved again it is still Forgejo's, there to come back when Labs does.
    d.stop();
    let mut again: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut providers = vec![];
    block_configs(&mut again, &mut |c| providers.push(c["provider"].clone()));
    assert_eq!(providers, [json!("forgejo")], "{again}");
}

#[test]
fn the_forgejo_and_gitlab_hook_routes_are_not_there_without_labs() {
    use crate::forge_live::deliver;
    let d = daemon();
    let unknown = deliver(&d, "/api/forge/hooks/nowhere?k=x", &[], "{}");
    for path in ["/api/forge/hooks/forgejo?k=x", "/api/forge/hooks/gitlab?k=x"] {
        let status = deliver(&d, path, &[("X-Gitlab-Token", "x")], "{}");
        if cfg!(feature = "labs") {
            // Open to a forge without a login (a delivery is trusted by its
            // signature), so Labs off answers 404 itself.
            assert_eq!(status, 404, "{path}");
        } else {
            // No route, and so not let past the guard either.
            assert_eq!(status, unknown, "{path}");
        }
    }
    #[cfg(feature = "labs")]
    {
        // The switch is read on each delivery: with it on the route is there
        // and takes a delivery only if it is signed.
        std::fs::write(d.state.join("labs"), "").unwrap();
        for path in ["/api/forge/hooks/forgejo?k=x", "/api/forge/hooks/gitlab?k=x"] {
            assert_eq!(deliver(&d, path, &[("X-Gitlab-Token", "x")], "{}"), 401, "{path}");
        }
        std::fs::remove_file(d.state.join("labs")).unwrap();
        assert_eq!(deliver(&d, "/api/forge/hooks/forgejo?k=x", &[], "{}"), 404);
    }
}
