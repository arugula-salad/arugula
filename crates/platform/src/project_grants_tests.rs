//! A project's Access view (Arugula #682) through the platform's routes,
//! against the test hub: who may list it (its members, by the roster the
//! cache holds), what it says of `cloud.drive`, which rows do nothing and
//! why, and what it never shows (another account's grants over their DMs,
//! a revoked grant, another project's, a token's secret, a token's or a
//! grant's id to anyone but an owner, a former member's name). The rig is
//! [`crate::agents_tests::seeded`]: a owns team t (b edits, x views) and
//! the machine geek; c alone is in team u.

use arugula_e2e::{
    hosted::{Delegation, HostedKey},
    now_ms,
    passkey::Assertion,
    project_grant::ProjectGrant,
    team::TeamRole,
};
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::sql::{Sql as _, params},
    db::{
        agent_tokens::NewToken,
        projects::{BOARDS_WRITE, ISSUES_WRITE, THREADS_POST, THREADS_READ},
    },
    project_grants::{IDLE_LEFT, IDLE_LOCKED_OUT, IDLE_NOT_THEIRS},
};

async fn access(base: &str, who: &str, project: &str) -> (u16, Value) {
    call(base, who, "GET", &format!("/api/projects/{project}/grants"), None).await
}

/// A machine's signed call to the view.
async fn as_machine(base: &str, who: &str, machine: &str, project: &str) -> u16 {
    reqwest::Client::new()
        .get(format!("{base}/api/projects/{project}/grants"))
        .header("x-test-daemon", format!("{who} {machine}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// Each power row: the subject's name ("" when unnamed), the giver's
/// account (or "" for a former member), the power, and why it does less
/// than the power ("" when it does it all).
fn powers(v: &Value) -> Vec<(String, String, String, String)> {
    v["powers"]["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["subject"]["name"].as_str().unwrap_or_default().to_owned(),
                g["owner"]["account"].as_str().unwrap_or_default().to_owned(),
                g["power"].as_str().unwrap().to_owned(),
                g["limited"]["why"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// Each roster row: the account, the role, and whether they're locked
/// out now.
fn roles(v: &Value) -> Vec<(String, String, bool)> {
    v["roles"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (m["account"].as_str().unwrap().to_owned(), m["role"].as_str().unwrap().to_owned(), m["locked_out"] == true)
        })
        .collect()
}

/// Each cloud.drive row: the member, their role, their machines, how many
/// drive.
fn drives(v: &Value) -> Vec<(String, String, u64, u64)> {
    v["cloud_drive"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["owner"]["account"].as_str().unwrap().to_owned(),
                m["role"].as_str().unwrap().to_owned(),
                m["machines"].as_u64().unwrap(),
                m["driving"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn s(x: &str) -> String {
    x.to_owned()
}

/// Every member lists the same view: the roster's roles, every member's
/// machines driving by `cloud.drive` (as their role allows, and never
/// "none" while that's so), and every live grant over the project,
/// whoever gave it, with whose it is. A stranger gets no such project, as
/// the project's other routes say; a machine's signature isn't a person's.
/// A grant over a's DMs, and one over another project, are never in it; a
/// revoked one goes at once; a machine whose `cloud.drive` its owner
/// revoked stops counting as driving.
#[tokio::test]
async fn members_list_the_projects_grants_and_nobody_else_does() {
    let (hub, _app, base, t, u) = seeded().await;
    hub.daemon("bobs", "b", Some("t"));
    hub.daemon("bobs2", "b", None);
    hub.daemon("xbox", "x", None);
    let project = |id: &str| json!({ "kind": "project", "id": id });
    for (who, machine, power, scope) in [
        ("a", "geek", THREADS_READ, project(&t)),
        ("a", "geek", THREADS_READ, json!({ "kind": "account" })),
        ("b", "bobs", THREADS_POST, project(&t)),
    ] {
        let (st, v) = call(&base, who, "POST", "/api/grants", give(machine, power, scope)).await;
        assert_eq!(st, 200, "{v}");
    }

    let (st, owner) = access(&base, "a", &t).await;
    assert_eq!(st, 200, "{owner}");
    assert_eq!(owner["role"], "owner");
    assert_eq!(owner["project"]["id"], t);
    assert_eq!(owner["project"]["owner"]["kind"], "team");
    assert_eq!(roles(&owner), [("a", "owner"), ("b", "editor"), ("x", "viewer")].map(|(a, r)| (s(a), s(r), false)));
    assert_eq!(
        powers(&owner),
        [("geek-box", "a", THREADS_READ, ""), ("bobs-box", "b", THREADS_POST, "")].map(|(m, o, p, i)| (
            s(m),
            s(o),
            s(p),
            s(i)
        )),
        "the project's grants only: a's over their DMs isn't here"
    );
    // cloud.drive: every member's own machines, however many, a row
    // each, a viewer's too (its machine reads what a viewer reads).
    assert_eq!(
        drives(&owner),
        [("a", "owner", 1, 1), ("b", "editor", 2, 2), ("x", "viewer", 1, 1)].map(|(a, r, m, d)| (s(a), s(r), m, d))
    );
    assert!(
        owner["cloud_drive"]["says"].as_str().unwrap().contains("A viewer's reads what a viewer reads and can wake")
    );
    assert!(owner["cloud_drive"]["says"].as_str().unwrap().contains("nobody revokes it per project"), "{owner}");
    assert!(owner["roles"]["says"].as_str().unwrap().contains("roster"), "{owner}");
    assert!(owner["powers"]["says"].as_str().unwrap().contains("platform"), "{owner}");
    // a's grant over their DMs is in a's own list and nowhere in the
    // view, by its id; and no cloud.drive grant is listed as a power.
    let (_, as_own) = call(&base, "a", "GET", "/api/grants", None).await;
    let own = as_own["grants"].as_array().unwrap();
    let dms_id = own.iter().find(|g| g["scope"]["kind"] == "account" && g["power"] == THREADS_READ).unwrap()["id"]
        .as_str()
        .unwrap();
    let drive_id = own.iter().find(|g| g["power"] == "cloud.drive").unwrap()["id"].as_str().unwrap();
    let text = owner.to_string();
    assert!(!text.contains(dms_id) && !text.contains(drive_id), "{text}");
    assert!(owner["powers"]["grants"].as_array().unwrap().iter().all(|g| g["power"] != "cloud.drive"));
    assert_eq!(owner["powers"]["grants"].as_array().unwrap().iter().filter(|g| g["power"] == THREADS_READ).count(), 1);
    // The owner sees ids, to revoke: the grant's and the machine's.
    assert!(owner["powers"]["grants"][0]["id"].as_str().unwrap().starts_with("grt_"), "{owner}");
    assert_eq!(owner["powers"]["grants"][0]["subject"]["id"], "geek");

    // An editor and a viewer read the same list, without the ids.
    for who in ["b", "x"] {
        let (st, v) = access(&base, who, &t).await;
        assert_eq!(st, 200, "{who}: {v}");
        assert_eq!(powers(&v), powers(&owner), "{who}");
        assert_eq!(roles(&v), roles(&owner), "{who}");
        assert_eq!(drives(&v), drives(&owner), "{who}");
        assert_eq!(v["role"], if who == "b" { "editor" } else { "viewer" });
        assert_eq!(v["powers"]["grants"][0]["id"], Value::Null, "{who}: a grant's id is an owner's");
        assert_eq!(v["powers"]["grants"][0]["subject"]["id"], Value::Null, "{who}: so is a machine's");
        assert!(!v.to_string().contains("grt_") && !v.to_string().contains("\"geek\""), "{who}: {v}");
    }
    // A stranger: no such project, the same words the repos route uses.
    let (st, v) = access(&base, "c", &t).await;
    let (st_repos, repos) = call(&base, "c", "GET", &format!("/api/projects/{t}/repos"), None).await;
    assert_eq!((st, st_repos), (404, 404), "{v} {repos}");
    assert_eq!(v["error"], repos["error"]);
    assert_eq!(access(&base, "c", "prj_00000000000000000000000000000000").await.0, 404);
    // u's view has nothing of t's.
    let (st, v) = access(&base, "c", &u).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(powers(&v), []);
    assert_eq!(roles(&v), [(s("c"), s("owner"), false)]);
    assert_eq!(drives(&v), [(s("c"), s("owner"), 0, 0)]);
    // A machine's signature isn't a person's.
    assert_eq!(as_machine(&base, "a", "geek", &t).await, 401);
    // Nobody signed in at all.
    let r = reqwest::Client::new().get(format!("{base}/api/projects/{t}/grants")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // A revoke goes at once; b revoking bobs2's cloud.drive takes it out
    // of the driving count, the machine stays b's.
    let (_, mine) = call(&base, "b", "GET", "/api/grants", None).await;
    let find = |power: &str, subject: &str| {
        mine["grants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["power"] == power && g["subject"]["id"] == subject)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for id in [find(THREADS_POST, "bobs"), find("cloud.drive", "bobs2")] {
        assert_eq!(call(&base, "b", "POST", &format!("/api/grants/{id}/revoke"), None).await.0, 200);
    }
    let (_, v) = access(&base, "x", &t).await;
    assert_eq!(powers(&v).len(), 1, "{v}");
    assert_eq!(powers(&v)[0].0, "geek-box");
    assert_eq!(drives(&v)[1], (s("b"), s("editor"), 2, 1), "{v}");
}

/// A grant whose giver left the project, revoked by an owner through
/// `POST /api/grants/{id}/revoke`: the answer names neither the giver nor
/// their machine, now or when it's revoked again; a grant whose giver is
/// still in the project answers as it always did.
#[tokio::test]
async fn an_owners_revoke_of_a_former_members_grant_names_nobody() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("bobs", "b", Some("t"));
    let (st, g) =
        call(&base, "b", "POST", "/api/grants", give("bobs", THREADS_POST, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, g2) =
        call(&base, "b", "POST", "/api/grants", give("bobs", THREADS_READ, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g2}");
    // While b is in: the owner's revoke names the machine and the giver.
    let (st, r) = call(&base, "a", "POST", &format!("/api/grants/{}/revoke", g2["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(
        (&r["granted_by"], &r["subject"]["name"], &r["former_giver"]),
        (&json!("b"), &json!("bobs-box"), &Value::Null)
    );
    // b leaves. The owner revokes the other: no name, no id, no account.
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let path = format!("/api/grants/{}/revoke", g["id"].as_str().unwrap());
    for again in [false, true] {
        let (st, r) = call(&base, "a", "POST", &path, None).await;
        assert_eq!(st, 200, "again {again}: {r}");
        assert_eq!(r["former_giver"], true, "{r}");
        assert_eq!(
            (&r["granted_by"], &r["subject"]["id"], &r["subject"]["name"]),
            (&Value::Null, &Value::Null, &Value::Null),
            "{r}"
        );
        assert_eq!((&r["live"], &r["power"]), (&json!(false), &json!(THREADS_POST)));
        let text = r.to_string();
        for word in ["\"b\"", "bobs", "bobs-box"] {
            assert!(!text.contains(word), "again {again}: {word} names a former member: {text}");
        }
    }
    // The grant is revoked for real.
    assert_eq!(app.db.live_grant("bobs", THREADS_POST, ("project", &t), "b").unwrap(), None);
}

/// Who an owner's revoke names, and who it doesn't: a giver locked out of
/// a locked team is still on the roster, so named, not a former member; a
/// giver revoking their own grant after leaving is never unnamed to
/// themselves; a grant its giver revoked before leaving answers without
/// naming them as its revoker either; and a revoker who has since left
/// (a former co-owner) is unnamed too.
#[tokio::test]
async fn an_owners_revoke_names_whoever_is_still_on_the_roster() {
    let (hub, app, base, t, _u) = seeded().await;
    let roster = |members: &[(&str, TeamRole)]| {
        hub.team("t", members);
    };
    let caught_up = || async {
        crate::cache::catch_up(&app).await.unwrap();
        crate::projects::follow(&app).await.unwrap();
    };
    hub.daemon("bobs", "b", Some("t"));
    let mut ids = vec![];
    for power in [THREADS_POST, THREADS_READ, BOARDS_WRITE, ISSUES_WRITE] {
        let (st, g) =
            call(&base, "b", "POST", "/api/grants", give("bobs", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
        ids.push(g["id"].as_str().unwrap().to_owned());
    }
    let revoke = |who: &'static str, id: String| {
        let base = base.clone();
        async move { call(&base, who, "POST", &format!("/api/grants/{id}/revoke"), None).await }
    };

    // Locked: b is out of the project, but on its roster. Named.
    hub.lock("t", true);
    caught_up().await;
    let (st, r) = revoke("a", ids[0].clone()).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(
        (&r["granted_by"], &r["subject"]["name"], &r["former_giver"]),
        (&json!("b"), &json!("bobs-box"), &Value::Null)
    );
    assert_eq!(r["revoked_by"]["account"], "a");
    hub.lock("t", false);
    caught_up().await;

    // b revokes one of their own, then leaves; a revokes it again: the
    // answer names neither b as its giver nor b as its revoker.
    let (st, r) = revoke("b", ids[1].clone()).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("b")), "{r}");
    // o, an owner, revokes another of b's, then o leaves too.
    roster(&[("a", TeamRole::Owner), ("o", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    caught_up().await;
    let (st, r) = revoke("o", ids[2].clone()).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("o")), "{r}");
    roster(&[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    caught_up().await;
    let (st, r) = revoke("a", ids[1].clone()).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["former_giver"], true, "{r}");
    assert_eq!(r["revoked_by"], json!({ "account": null, "name": "a former member" }), "{r}");
    assert!(!r.to_string().contains("\"b\"") && !r.to_string().contains("bobs"), "{r}");
    let (st, r) = revoke("a", ids[2].clone()).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["revoked_by"], json!({ "account": null, "name": "a former member" }), "o has left: {r}");
    assert!(!r.to_string().contains("\"o\""), "{r}");

    // b, gone from the project, revokes their own last grant: b is told
    // about it as b, not as a former member to themselves.
    let (st, r) = revoke("b", ids[3].clone()).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(
        (&r["granted_by"], &r["subject"]["name"], &r["former_giver"]),
        (&json!("b"), &json!("bobs-box"), &Value::Null)
    );
    assert_eq!(r["revoked_by"]["account"], "b");
}

/// The roster is read through the cache, as `agent_may` reads it: a
/// second list asks the hub nothing but the project's delegation (the one
/// thing the cache doesn't keep); a member taken off the roster still
/// reads it until the feed brings the change, and is a stranger from
/// then on; a locked team's editors and viewers are out while it's
/// locked. A grant whose giver left does nothing and says so, with the
/// giver a former member, unnamed; a demoted viewer's write power is
/// limited by the one definition the grants list uses.
#[tokio::test]
async fn the_roster_comes_through_the_cache() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("bobs", "b", Some("t"));
    for power in [THREADS_POST, ISSUES_WRITE] {
        let (st, v) =
            call(&base, "b", "POST", "/api/grants", give("bobs", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{v}");
    }
    assert_eq!(access(&base, "b", &t).await.0, 200);
    let (before, delegations) = (hub.calls(), hub.calls_of("live_delegation"));
    let (st, v) = access(&base, "b", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(hub.calls_of("live_delegation"), delegations + 1, "the delegation is asked each time");
    assert_eq!(hub.calls(), before + 1, "and nothing else: the roster, machines and names are the cache's");
    assert_eq!(
        powers(&v),
        [("bobs-box", "b", ISSUES_WRITE, ""), ("bobs-box", "b", THREADS_POST, "")].map(|(m, o, p, i)| (
            s(m),
            s(o),
            s(p),
            s(i)
        ))
    );

    // b is demoted to viewer: issues.write does nothing, threads.post
    // still does, and the rows say so as `GET /api/grants` does.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let (_, v) = access(&base, "x", &t).await;
    let rows = powers(&v);
    assert_eq!((rows[0].2.as_str(), rows[1].2.as_str()), (ISSUES_WRITE, THREADS_POST), "{v}");
    assert!(rows[0].3.contains("does nothing here: its giver only views"), "{v}");
    assert_eq!(rows[1].3, "", "{v}");
    let (_, mine) = call(&base, "b", "GET", "/api/grants", None).await;
    let theirs = mine["grants"].as_array().unwrap().iter().find(|g| g["power"] == ISSUES_WRITE).unwrap();
    assert_eq!(theirs["limited"], v["powers"]["grants"][0]["limited"], "one definition: the grants list's");
    assert_eq!(v["powers"]["grants"][0]["limited"]["label"], "idle");
    assert_eq!(v["powers"]["grants"][0]["owner_role"], "viewer");

    // The team is locked: only its owners are in it. b and x are out
    // (404, no such project), and the view a sees names them as locked
    // out, not former: on the roster, with their roles, and b's grants
    // doing nothing while the lock holds, b named.
    hub.lock("t", true);
    crate::cache::catch_up(&app).await.unwrap();
    for who in ["b", "x"] {
        assert_eq!(access(&base, who, &t).await.0, 404, "{who} is out while the team is locked");
    }
    let (st, v) = access(&base, "a", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        roles(&v),
        [("a", "owner", false), ("b", "viewer", true), ("x", "viewer", true)].map(|(a, r, l)| (s(a), s(r), l))
    );
    assert_eq!(v["project"]["owner"]["locked"], true);
    assert_eq!(drives(&v), [(s("a"), s("owner"), 1, 1)], "only a drives while the team is locked");
    let rows = powers(&v);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.1 == "b" && r.3 == IDLE_LOCKED_OUT), "{v}");
    let g = &v["powers"]["grants"][0];
    assert_eq!(g["owner"], json!({ "account": "b", "name": "b", "former": false, "locked_out": true }), "{v}");
    assert_eq!(g["subject"]["name"], "bobs-box", "a locked-out member's machine is still named");
    hub.lock("t", false);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(access(&base, "x", &t).await.0, 200);

    // b leaves the team. Until the feed brings it, b still reads the
    // view (the cache's roster; `agent_may` reads the same); once it
    // does, b is a stranger, and b's grants stay and do nothing, their
    // giver a former member with no name.
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    assert_eq!(access(&base, "b", &t).await.0, 200, "before the feed: the cache's roster");
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    assert_eq!(access(&base, "b", &t).await.0, 404);
    let (st, v) = access(&base, "x", &t).await;
    assert_eq!(st, 200, "{v}");
    let rows = powers(&v);
    assert_eq!(rows.len(), 2, "{v}");
    assert!(rows.iter().all(|r| r.0.is_empty() && r.1.is_empty() && r.3 == IDLE_LEFT), "{v}");
    for g in v["powers"]["grants"].as_array().unwrap() {
        assert_eq!(g["owner"], json!({ "account": null, "name": null, "former": true, "locked_out": false }), "{v}");
        assert_eq!(g["owner_role"], Value::Null);
        assert_eq!(
            (&g["subject"]["id"], &g["subject"]["name"]),
            (&Value::Null, &Value::Null),
            "nor their machine: {v}"
        );
        assert_eq!(g["id"], Value::Null, "x isn't an owner");
    }
    // Nothing of b's is in the answer: not the account, not the machine's
    // id or name, by any key.
    let text = v.to_string();
    for word in ["\"b\"", "bobs", "bobs-box"] {
        assert!(!text.contains(word), "{word} names a former member: {text}");
    }
    // An owner's view hides them the same way, ids included.
    let (_, as_a) = access(&base, "a", &t).await;
    let text = as_a.to_string();
    for word in ["\"b\"", "bobs", "bobs-box"] {
        assert!(!text.contains(word), "{word} names a former member to an owner: {text}");
    }
    assert!(
        as_a["powers"]["grants"][0]["id"].as_str().unwrap().starts_with("grt_"),
        "the owner still gets the grant's id"
    );
    assert_eq!(roles(&v).len(), 2);
    assert_eq!(drives(&v).len(), 2, "no cloud.drive row for someone who left");

    // A machine that's no longer its giver's: the grant does nothing,
    // and the machine isn't named (it's someone else's now).
    hub.daemon("bobs", "x", Some("t"));
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    for who in ["x", "a"] {
        let (_, v) = access(&base, who, &t).await;
        assert!(powers(&v).iter().all(|r| r.0.is_empty() && r.1 == "b" && r.3 == IDLE_NOT_THEIRS), "{who}: {v}");
        assert!(v["powers"]["grants"].as_array().unwrap().iter().all(|g| g["subject"]["id"].is_null()), "{who}: {v}");
    }
}

/// An agent grant token's powers over the project are listed to every
/// member with whose the token is, by its name, never its secret or
/// hash; its id (what a revoke needs) to the project's owners alone; a
/// revoked token's aren't listed. A personal project reads as its one
/// owner, and nobody else's.
#[tokio::test]
async fn tokens_show_by_name_and_a_personal_project_by_its_owner() {
    let (_hub, app, base, t, _u) = seeded().await;
    let token = |id: &str, name: &str| NewToken {
        id: id.into(),
        account: "b".into(),
        name: name.into(),
        token_hash: format!("hash-of-{id}"),
        minted_from: "a browser".into(),
    };
    app.db.mint_agent_token(&token("agt_live", "ci"), &[(BOARDS_WRITE, &t)], 1).unwrap();
    app.db.mint_agent_token(&token("agt_gone", "old"), &[(ISSUES_WRITE, &t)], 1).unwrap();
    app.db.revoke_agent_token("b", "agt_gone", 2).unwrap();
    let hub = _hub;
    for who in ["x", "b", "a"] {
        let (st, v) = access(&base, who, &t).await;
        assert_eq!(st, 200, "{v}");
        let g = &v["powers"]["grants"];
        assert_eq!(g.as_array().unwrap().len(), 1, "{v}");
        assert_eq!(g[0]["subject"]["kind"], "agent");
        assert_eq!(g[0]["subject"]["name"], "ci (MCP)");
        assert_eq!(
            (&g[0]["owner"]["account"], &g[0]["power"], &g[0]["limited"]),
            (&json!("b"), &json!(BOARDS_WRITE), &Value::Null)
        );
        assert!(!v.to_string().contains("hash-of"), "{v}");
        if who == "a" {
            assert_eq!(g[0]["subject"]["id"], "agt_live", "an owner sees the id, to revoke");
            assert!(g[0]["id"].as_str().unwrap().starts_with("grt_"));
        } else {
            assert_eq!((&g[0]["subject"]["id"], &g[0]["id"]), (&Value::Null, &Value::Null), "{who}: no ids");
            assert!(!v.to_string().contains("agt_") && !v.to_string().contains("grt_"), "{who}: {v}");
        }
    }

    // b is made a viewer: boards.write on b's token still reads boards,
    // and the row says so with the grants list's own label and words.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let (_, v) = access(&base, "x", &t).await;
    let l = &v["powers"]["grants"][0]["limited"];
    assert_eq!(l["label"], "reads only", "{v}");
    assert!(l["why"].as_str().unwrap().contains("reads its boards"), "{v}");
    // A grant on b's token that someone else gave (a row the routes never
    // write; the database can): the token isn't its giver's, so it does
    // nothing, and the token isn't named.
    app.db
        .c()
        .execute(
            "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
             VALUES ('grt_00000000000000000000000000000001', 'agent', 'agt_live', ?1, 'project', ?2, 'x', 3)",
            params![THREADS_READ, t],
        )
        .unwrap();
    let (_, v) = access(&base, "a", &t).await;
    let odd = v["powers"]["grants"].as_array().unwrap().iter().find(|g| g["power"] == THREADS_READ).unwrap();
    assert_eq!(odd["limited"]["why"], crate::project_grants::IDLE_TOKEN_GONE, "{v}");
    assert_eq!(
        (&odd["subject"]["id"], &odd["subject"]["name"], &odd["owner"]["account"]),
        (&Value::Null, &Value::Null, &json!("x"))
    );
    // b leaves: the platform revokes b's token's grants over the team's
    // projects with the roster (`revoke_departed`), so b's own row goes;
    // the odd one x gave stays, and names neither b nor the token, to
    // anyone.
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    for who in ["x", "a"] {
        let (_, v) = access(&base, who, &t).await;
        let rows = v["powers"]["grants"].as_array().unwrap();
        assert!(rows.iter().all(|g| g["power"] != BOARDS_WRITE), "{who}: b's token's own grant went with b: {v}");
        let g = rows.iter().find(|g| g["power"] == THREADS_READ).unwrap();
        assert_eq!((&g["subject"]["id"], &g["subject"]["name"]), (&Value::Null, &Value::Null), "{who}: {v}");
        let text = v.to_string();
        for word in ["agt_live", "\"ci", "\"b\""] {
            assert!(!text.contains(word), "{who}: {word} names a former member's token: {text}");
        }
    }
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();

    // a's personal project: a alone, as its owner; c can't read it.
    let personal = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    let (st, v) = access(&base, "a", &personal).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["project"]["owner"], json!({ "kind": "account", "id": "a" }));
    assert_eq!(roles(&v), [(s("a"), s("owner"), false)]);
    assert_eq!(drives(&v), [(s("a"), s("owner"), 1, 1)]);
    assert!(v["roles"]["says"].as_str().unwrap().contains("own project"), "{v}");
    assert_eq!(powers(&v), []);
    assert_eq!(access(&base, "c", &personal).await.0, 404);
    assert_eq!(access(&base, "b", &personal).await.0, 404, "a teammate isn't in a's personal project");
    // `personal` names the caller's own.
    let (st, mine) = access(&base, "c", "personal").await;
    assert_eq!(st, 200, "{mine}");
    assert_eq!(roles(&mine), [(s("c"), s("owner"), false)]);
}

/// What an owner allowed: the delegation as control says it, and the
/// standing run grant with its expiry and who confirmed it (never its
/// bytes or assertion), stale once control's live delegation is another;
/// and the project's cloud workspaces, each saying that an editor has
/// full use. The view is rate-limited per account.
#[tokio::test]
async fn allow_and_workspaces_say_what_an_owner_signed() {
    let (hub, app, base, t, _u) = seeded().await;
    let (st, v) = access(&base, "x", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["allow"]["delegation"], json!({ "live": false, "id": null }));
    assert_eq!(v["allow"]["run_grant"], Value::Null);
    assert_eq!(v["workspaces"]["list"], json!([]));

    let d = Delegation {
        project: t.clone(),
        owner: "team:t".to_owned(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    hub.set_delegation(&t, Some(&d.id()));
    let now = now_ms();
    let mut g = ProjectGrant::v2(&d, "a", now, now + 86_400_000);
    g.passkey = Some(Assertion {
        id: "pk-a".into(),
        client_data_json: "cd".into(),
        authenticator_data: "ad".into(),
        signature: "sig".into(),
    });
    app.db.add_run_grant(&g, "a", now).unwrap();
    app.db.add_hosted_workspace("hw_1", &t, "dev", &Default::default(), "b", 10, now).unwrap();

    let (st, v) = access(&base, "x", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["allow"]["delegation"], json!({ "live": true, "id": d.id() }));
    let rg = &v["allow"]["run_grant"];
    assert_eq!(
        (&rg["id"], &rg["by"]["account"], &rg["by"]["name"], &rg["expires"]),
        (&json!(g.id), &json!("a"), &json!("a"), &json!(now + 86_400_000))
    );
    assert_eq!((&rg["stale"], &rg["signer_owner"]), (&json!(false), &json!(true)));
    assert_eq!(
        rg.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["by", "created_at", "expires", "id", "made", "signer_owner", "stale"]
    );
    let ws = &v["workspaces"]["list"][0];
    assert_eq!((&ws["name"], &ws["created_by"]["account"]), (&json!("dev"), &json!("b")));
    assert!(ws["full_use"].as_str().unwrap().contains("editors have full use"), "{v}");
    assert!(v["workspaces"]["says"].as_str().unwrap().contains("checks it itself"), "{v}");
    assert!(v["allow"]["says"].as_str().unwrap().contains("passkey"), "{v}");
    assert!(!v.to_string().contains("\"sig\"") && !v.to_string().contains("\"body\""), "{v}");

    // An owner delegated the project again: the grant names the earlier
    // delegation, so machines enrolled since refuse it. The view says
    // stale, as the runs settings do (`run_grant::show`).
    hub.set_delegation(&t, Some("dlg_newer"));
    let (_, v) = access(&base, "a", &t).await;
    assert_eq!(v["allow"]["run_grant"]["stale"], true, "{v}");
    let (_, shown) = call(&base, "a", "GET", &format!("/api/projects/{t}/runs/grant"), None).await;
    assert_eq!(shown["stale"], true, "the same check");
    // Control has no live delegation any more: not stale (there's nothing
    // newer), as the runs settings say.
    hub.set_delegation(&t, None);
    let (_, v) = access(&base, "a", &t).await;
    assert_eq!(v["allow"]["delegation"], json!({ "live": false, "id": null }));
    assert_eq!(v["allow"]["run_grant"]["stale"], false);

    // The owner who confirmed the grant stops being one: a machine refuses
    // the grant (NotAnOwner), so the view says the signer isn't an owner.
    // Here o signs it; o is demoted, then leaves, and the workspace's
    // maker leaves too. The signer reads as a former member, unnamed; no
    // former member's name is anywhere in the answer, the maker's row
    // included. (Its exact shape for a maker who left, `created_by` null,
    // is a separate change's to set and test.) While the team is locked,
    // full use is the owners' alone.
    hub.team("t", &[("a", TeamRole::Owner), ("o", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let mut by_o = ProjectGrant::v2(&d, "o", now + 1, now + 86_400_000);
    by_o.passkey = g.passkey.clone();
    app.db.add_run_grant(&by_o, "o", now + 1).unwrap();
    let (_, v) = access(&base, "a", &t).await;
    let rg = &v["allow"]["run_grant"];
    assert_eq!((&rg["by"]["account"], &rg["signer_owner"]), (&json!("o"), &json!(true)), "{v}");
    let (_, shown) = call(&base, "a", "GET", &format!("/api/projects/{t}/runs/grant"), None).await;
    assert_eq!(shown["signer_gone"], false, "{shown}");
    hub.team("t", &[("a", TeamRole::Owner), ("o", TeamRole::Editor), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let (_, v) = access(&base, "a", &t).await;
    let rg = &v["allow"]["run_grant"];
    assert_eq!((&rg["by"]["account"], &rg["signer_owner"]), (&json!("o"), &json!(false)), "{v}");
    // The runs settings say the same, by the same check: sign it again.
    let (_, shown) = call(&base, "a", "GET", &format!("/api/projects/{t}/runs/grant"), None).await;
    assert_eq!((&shown["signer_gone"], &shown["offer"]), (&json!(true), &json!(true)), "{shown}");
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    let (_, v) = access(&base, "a", &t).await;
    let rg = &v["allow"]["run_grant"];
    assert_eq!(rg["by"], json!({ "account": null, "name": null, "former": true, "locked_out": false }), "{v}");
    assert_eq!(rg["signer_owner"], false);
    let text = v.to_string();
    for word in ["\"o\"", "\"b\""] {
        assert!(!text.contains(word), "{word} names a former member: {text}");
    }
    hub.lock("t", true);
    crate::cache::catch_up(&app).await.unwrap();
    let (_, v) = access(&base, "a", &t).await;
    assert!(v["workspaces"]["full_use"].as_str().unwrap().starts_with("the team is locked"), "{v}");
    assert_eq!(v["workspaces"]["list"][0]["full_use"], v["workspaces"]["full_use"]);
    hub.lock("t", false);
    crate::cache::catch_up(&app).await.unwrap();

    // Rate-limited per account: once x's hour is spent (the reads above
    // counted), the next read is refused, and another account's aren't
    // counted.
    let mut spent = 0;
    while app.limits.check_account(crate::project_grants::ACCESS_READS, "x").is_ok() {
        spent += 1;
        assert!(spent <= crate::project_grants::ACCESS_READS.1, "the limit counts");
    }
    assert_eq!(access(&base, "x", &t).await.0, 429);
    assert_eq!(access(&base, "a", &t).await.0, 200);
}
