//! What the daemon tells the web client's hints (DECISIONS.md "Hints come
//! from facts, once per account"): a link to a PR printed in a pane is its
//! `link`, and a pane brought back after a restart whose process went with
//! the old daemon is `restored`.

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
