//! Renaming a machine (Arugula #500), over HTTP: who may, what a name must
//! be, that the machine's id is what stays, that the old name keeps
//! working, and that the machine reconnecting doesn't undo it.

use std::time::Duration;

use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::{Value, json};

use crate::{
    db::Team,
    routing_wire::{Control, control, daemon, dial_relay, member, person, roster, session},
};

async fn rename(c: &Control, cookie: &str, id: &str, name: &str) -> (u16, Value) {
    c.as_person(cookie, "POST", &format!("/api/daemons/{id}/name"), Some(json!({ "name": name }))).await
}

/// What the account's directory calls its machines, by id.
async fn names(c: &Control, cookie: &str) -> Vec<(String, String)> {
    let (_, dir) = c.as_person(cookie, "GET", "/api/directory", None).await;
    let mut out: Vec<(String, String)> = dir["daemons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["id"].as_str().unwrap().to_owned(), d["name"].as_str().unwrap().to_owned()))
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn an_owner_renames_a_machine_and_the_old_name_stays_an_alias() {
    let c = control(|_| {}).await;
    person(&c.app, "aaron", "a1");
    let cookie = session(&c.app, "a1");
    let mini = daemon(&c.app, "a1", "Aarons-Mac-mini.local");

    let (st, v) = rename(&c, &cookie, &mini.id(), "  Mini  ").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["name"].clone(), v["was"].clone()), (json!("Mini"), json!("Aarons-Mac-mini.local")));
    assert_eq!(names(&c, &cookie).await, [(mini.id(), "Mini".to_owned())], "the directory says it now");
    assert_eq!(c.app.db.daemon_aliases(&mini.id()).unwrap(), ["aarons-mac-mini.local"]);

    // Renamed again: both old words stay, the one it is called now doesn't.
    assert_eq!(rename(&c, &cookie, &mini.id(), "Box").await.0, 200);
    assert_eq!(rename(&c, &cookie, &mini.id(), "mini").await.0, 200);
    assert_eq!(c.app.db.daemon_aliases(&mini.id()).unwrap(), ["aarons-mac-mini.local", "box"]);
}

/// The trap: a daemon coming back with its hostname must not undo it.
#[tokio::test]
async fn a_machine_reconnecting_keeps_its_new_name() {
    let c = control(|_| {}).await;
    person(&c.app, "aaron", "a1");
    let cookie = session(&c.app, "a1");
    let mini = daemon(&c.app, "a1", "Aarons-Mac-mini.local");
    assert_eq!(rename(&c, &cookie, &mini.id(), "mini").await.0, 200);

    // It dials the relay again (and again), and asks for the directory and
    // the trust as it does on each start, as itself.
    for _ in 0..2 {
        let socket = dial_relay(&c, &mini).await;
        for _ in 0..50 {
            if c.app.relay.online(&mini.id()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(c.app.relay.online(&mini.id()));
        assert_eq!(c.daemon_post(&mini, "/api/daemon/access", &json!({ "accounts": [] })).await.0, 200);
        drop(socket);
    }
    c.app.db.seen(&mini.id(), Some(&["http://10.0.0.2:7683".to_owned()]), now_ms()).unwrap();
    assert_eq!(names(&c, &cookie).await, [(mini.id(), "mini".to_owned())]);
    assert_eq!(c.app.db.daemon_aliases(&mini.id()).unwrap(), ["aarons-mac-mini.local"]);
}

#[tokio::test]
async fn a_name_is_trimmed_short_mentionable_and_unique_among_the_owners_machines() {
    let c = control(|_| {}).await;
    person(&c.app, "aaron", "a1");
    person(&c.app, "jake", "j1");
    let cookie = session(&c.app, "a1");
    let mini = daemon(&c.app, "a1", "Aarons-Mac-mini.local");
    let pad = daemon(&c.app, "a1", "pad");
    let theirs = daemon(&c.app, "j1", "geek");

    for bad in ["", "   ", "!!!", &"x".repeat(41), "two\nlines"] {
        let (st, v) = rename(&c, &cookie, &mini.id(), bad).await;
        assert_eq!(st, 400, "{bad:?}: {v}");
    }
    assert_eq!(rename(&c, &cookie, &mini.id(), &"x".repeat(40)).await.0, 200, "40 is the longest");
    assert_eq!(rename(&c, &cookie, &mini.id(), "Aarons-Mac-mini.local").await.0, 200, "and back");

    // The same word as another of its owner's machines: refused, whatever the
    // case or the punctuation, with the other's name in the message.
    for same in ["pad", "PAD", " Pad ", "pad!"] {
        let (st, v) = rename(&c, &cookie, &mini.id(), same).await;
        assert_eq!(st, 409, "{same:?}: {v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("already called pad"), "{v}");
    }
    // Its own word again, or another account's machine's name, is fine.
    assert_eq!(rename(&c, &cookie, &pad.id(), "PAD").await.0, 200);
    assert_eq!(rename(&c, &cookie, &mini.id(), "geek").await.0, 200);
    assert_eq!(names(&c, &cookie).await.len(), 2);
    assert_eq!(c.app.db.daemon_account(&theirs.id()).unwrap().as_deref(), Some("j1"));
    assert_eq!(rename(&c, &cookie, "no-such-machine", "x").await.0, 404);
}

/// Only its owner renames it, or its team's owners: a stranger, and a team's
/// other members, can't. The id is who it is: nothing about it changes.
#[tokio::test]
async fn only_its_owner_or_its_teams_owners_rename_it() {
    let c = control(|_| {}).await;
    let fran = person(&c.app, "fran", "f1");
    let cy = person(&c.app, "cy", "c1");
    let mo = person(&c.app, "mo", "m1");
    let ed = person(&c.app, "ed", "e1");
    person(&c.app, "ox", "x1");
    let team = "0011223344556677";
    let r = roster(
        team,
        1,
        vec![
            member("f1", &fran, TeamRole::Owner),
            member("c1", &cy, TeamRole::Editor),
            member("m1", &mo, TeamRole::Owner),
            member("e1", &ed, TeamRole::Editor),
        ],
        &fran,
    );
    let t = Team { id: team.into(), name: "Acme".into(), founder: "f1".into(), founder_root: fran.id(), locked: false };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    // Cy's machine, in the team; and another of Cy's, not.
    let cys = daemon(&c.app, "c1", "cys-box");
    c.app.db.set_daemon_team(&cys.id(), team).unwrap();
    let own = daemon(&c.app, "c1", "cys-own");

    let [fran_s, cy_s, mo_s, ed_s, ox_s] = ["f1", "c1", "m1", "e1", "x1"].map(|a| session(&c.app, a));
    // A stranger; another member who doesn't own the team; and an owner of
    // the team, for a machine that isn't the team's.
    assert_eq!(rename(&c, &ox_s, &cys.id(), "theirs").await.0, 403);
    assert_eq!(rename(&c, &ed_s, &cys.id(), "from-ed").await.0, 403);
    assert_eq!(rename(&c, &fran_s, &own.id(), "from-fran").await.0, 403);
    let mut now: Vec<String> = names(&c, &cy_s).await.into_iter().map(|(_, n)| n).collect();
    now.sort();
    assert_eq!(now, ["cys-box", "cys-own"], "nothing renamed");

    // The team's owners, and the machine's own account.
    let (st, v) = rename(&c, &mo_s, &cys.id(), "from-mo").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(rename(&c, &fran_s, &cys.id(), "from-fran").await.0, 200);
    assert_eq!(rename(&c, &cy_s, &cys.id(), "from-cy").await.0, 200);
    assert_eq!(c.app.db.daemon_account(&cys.id()).unwrap().as_deref(), Some("c1"), "still Cy's");
    assert_eq!(c.app.db.daemon_team(&cys.id()).unwrap().as_deref(), Some(team), "still the team's");

    // Within the team, a name is unique across its owners' machines too.
    let fran_box = daemon(&c.app, "f1", "fran-box");
    c.app.db.set_daemon_team(&fran_box.id(), team).unwrap();
    let (st, v) = rename(&c, &fran_s, &fran_box.id(), "From-Cy").await;
    assert_eq!(st, 409, "{v}");
}

/// A team's room reaches every member's granted machines, personal ones
/// too: a word a teammate's machine goes by now is taken, whether or not
/// either machine is in the team. Old names don't count.
#[tokio::test]
async fn a_name_a_teammates_personal_machine_has_is_taken() {
    let c = control(|_| {}).await;
    let aaron = person(&c.app, "aaron", "a1");
    let jake = person(&c.app, "jake", "j1");
    person(&c.app, "ox", "x1");
    let team = "0011223344556677";
    let r = roster(team, 1, vec![member("a1", &aaron, TeamRole::Owner), member("j1", &jake, TeamRole::Editor)], &aaron);
    let t =
        Team { id: team.into(), name: "Acme".into(), founder: "a1".into(), founder_root: aaron.id(), locked: false };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    let mac = daemon(&c.app, "a1", "Aarons-Mac-mini.local");
    let geek = daemon(&c.app, "j1", "geek");
    let strangers = daemon(&c.app, "x1", "box");
    let (a_s, j_s, x_s) = (session(&c.app, "a1"), session(&c.app, "j1"), session(&c.app, "x1"));

    let (st, v) = rename(&c, &a_s, &mac.id(), "Geek").await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("already called geek"), "{v}");
    assert_eq!(c.app.db.daemon_aliases(&mac.id()).unwrap(), Vec::<String>::new(), "nothing changed");
    // Not a stranger's: no team, no clash.
    assert_eq!(rename(&c, &a_s, &mac.id(), "box").await.0, 200);
    // An old name isn't taken: Jake's machine was "geek", now it isn't.
    assert_eq!(rename(&c, &j_s, &geek.id(), "pad").await.0, 200);
    assert_eq!(rename(&c, &a_s, &mac.id(), "geek").await.0, 200);
    // And the other way round.
    assert_eq!(rename(&c, &j_s, &geek.id(), "GEEK").await.0, 409);
    let _ = (x_s, strangers);
}
