//! What the daemon tells the web client's hints (DECISIONS.md "Hints come
//! from facts, once per account"): a link to a PR printed in a pane is its
//! `link`, a pane brought back after a restart whose process went with
//! the old daemon is `restored`, and a commit or a checkout in a pane moves
//! its repository's `head` or `branch`.

use arugula_testkit::{Daemon, arugulad};
use serde_json::{Value, json};

fn pane(d: &Daemon, id: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap_or_default()
}

#[test]
fn a_printed_pr_link_and_a_restored_pane() {
    let mut d = arugulad!("hintfacts").start();
    let id = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    assert_eq!(pane(&d, id)["restored"], Value::Null, "a pane this daemon started");

    // `printf` is the shell's own: the link is only in its output, split
    // by the format so the typed line doesn't have it.
    let url = "https://github.com/o/r/pull/12";
    d.post(
        &format!("/api/panes/{id}/send"),
        json!({ "text": "printf 'Created %s/pull/12\\n' https://github.com/o/r", "enter": true }),
    );
    d.wait_for("the link", || pane(&d, id)["link"] == url);

    d.stop();
    d.start();
    d.wait_for("the pane back", || pane(&d, id)["restored"] == true);
}

#[test]
fn a_commit_moves_head_and_a_checkout_the_branch() {
    let dir = std::env::temp_dir().join(format!("ilg-hint-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=a", "-c", "user.email=a@example.com"])
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["commit", "-q", "--allow-empty", "-m", "one"]);

    let d = arugulad!("hintgit").start();
    let id = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let send = |text: &str| d.post(&format!("/api/panes/{id}/send"), json!({ "text": text, "enter": true }));
    send(&format!("cd {}", dir.display()));
    d.wait_for("the repository", || pane(&d, id)["branch"] == "main");
    let first = pane(&d, id)["head"].as_str().unwrap_or_default().to_owned();
    assert_eq!(first.len(), 7, "{:?}", pane(&d, id));

    send("git -c user.name=a -c user.email=a@example.com commit -q --allow-empty -m two");
    d.wait_for("the commit", || pane(&d, id)["head"].as_str().is_some_and(|h| h.len() == 7 && h != first));
    send("git checkout -q -b topic");
    d.wait_for("the branch", || pane(&d, id)["branch"] == "topic");
    std::fs::remove_dir_all(&dir).ok();
}
