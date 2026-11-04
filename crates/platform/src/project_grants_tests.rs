//! A project's Access view (Arugula #682) through the platform's routes,
//! against the test hub: who may list it (its members, by the roster the
//! cache holds), and what it never shows (another account's grants over
//! their DMs or account, a revoked grant, another project's, a token's
//! secret). The rig is [`crate::agents_tests::seeded`]: a owns team t (b
//! edits, x views) and the machine geek; c alone is in team u.

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
    db::{
        agent_tokens::NewToken,
        projects::{BOARDS_WRITE, ISSUES_WRITE, THREADS_POST, THREADS_READ},
    },
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

fn powers(v: &Value) -> Vec<(String, String, String, bool)> {
    v["powers"]["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["subject"]["name"].as_str().unwrap_or_default().to_owned(),
                g["owner"]["account"].as_str().unwrap().to_owned(),
                g["power"].as_str().unwrap().to_owned(),
                g["reaches"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn roles(v: &Value) -> Vec<(String, String)> {
    v["roles"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["account"].as_str().unwrap().to_owned(), m["role"].as_str().unwrap().to_owned()))
        .collect()
}

/// Every member lists the same view: the roster's roles, and every live
/// grant over the project, whoever gave it, with whose it is. A stranger
/// gets no such project, as the project's other routes say; a machine's
/// signature isn't a person's. A grant over a's DMs, and one over another
/// project, are never in it; a revoked one goes at once.
#[tokio::test]
async fn members_list_the_projects_grants_and_nobody_else_does() {
    let (hub, _app, base, t, u) = seeded().await;
    hub.daemon("bobs", "b", Some("t"));
    let project = |id: &str| json!({ "kind": "project", "id": id });
    for (who, machine, power, scope) in [
        ("a", "geek", THREADS_READ, project(&t)),
        ("a", "geek", THREADS_READ, json!({ "kind": "account" })),
        ("b", "bobs", ISSUES_WRITE, project(&t)),
    ] {
        let (st, v) = call(&base, who, "POST", "/api/grants", give(machine, power, scope)).await;
        assert_eq!(st, 200, "{v}");
    }
    let (st, owner) = access(&base, "a", &t).await;
    assert_eq!(st, 200, "{owner}");
    assert_eq!(owner["role"], "owner");
    assert_eq!(owner["project"]["id"], t);
    assert_eq!(owner["project"]["owner"]["kind"], "team");
    assert_eq!(roles(&owner), [("a", "owner"), ("b", "editor"), ("x", "viewer")].map(|(a, r)| (a.into(), r.into())));
    assert_eq!(
        powers(&owner),
        [("geek-box", "a", THREADS_READ, true), ("bobs-box", "b", ISSUES_WRITE, true)].map(|(m, o, p, r)| (
            m.into(),
            o.into(),
            p.into(),
            r
        )),
        "the project's grants only: a's over their DMs isn't here"
    );
    assert!(owner["roles"]["says"].as_str().unwrap().contains("roster"), "{owner}");
    assert!(owner["powers"]["says"].as_str().unwrap().contains("platform"), "{owner}");
    let text = owner.to_string();
    assert!(!text.contains("account\":\"account") && !text.contains("\"scope\""), "no account-scoped grant: {text}");

    // An editor and a viewer read the same list.
    for who in ["b", "x"] {
        let (st, v) = access(&base, who, &t).await;
        assert_eq!(st, 200, "{who}: {v}");
        assert_eq!(powers(&v), powers(&owner), "{who}");
        assert_eq!(roles(&v), roles(&owner), "{who}");
        assert_eq!(v["role"], if who == "b" { "editor" } else { "viewer" });
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
    assert_eq!(roles(&v), [("c".to_owned(), "owner".to_owned())]);
    // A machine's signature isn't a person's.
    assert_eq!(as_machine(&base, "a", "geek", &t).await, 401);
    // Nobody signed in at all.
    let r = reqwest::Client::new().get(format!("{base}/api/projects/{t}/grants")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // A revoke goes at once.
    let (_, mine) = call(&base, "b", "GET", "/api/grants", None).await;
    let id =
        mine["grants"].as_array().unwrap().iter().find(|g| g["power"] == ISSUES_WRITE).unwrap()["id"].as_str().unwrap();
    assert_eq!(call(&base, "b", "POST", &format!("/api/grants/{id}/revoke"), None).await.0, 200);
    let (_, v) = access(&base, "x", &t).await;
    assert_eq!(powers(&v).len(), 1, "{v}");
    assert_eq!(powers(&v)[0].0, "geek-box");
}

/// The roster is read through the cache, as `agent_may` reads it: a
/// second list asks the hub nothing, and a member taken off the roster
/// stops reading it once the feed brings the change. A grant whose giver
/// left reaches nothing, and says so.
#[tokio::test]
async fn the_roster_comes_through_the_cache() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("bobs", "b", Some("t"));
    let (st, v) =
        call(&base, "b", "POST", "/api/grants", give("bobs", THREADS_POST, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(access(&base, "b", &t).await.0, 200);
    let before = hub.calls_of("teams");
    let (st, v) = access(&base, "b", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(hub.calls_of("teams"), before, "the roster is the cache's");
    assert_eq!(powers(&v), [("bobs-box".to_owned(), "b".to_owned(), THREADS_POST.to_owned(), true)]);

    // b leaves the team: the hub's feed brings it, and b is a stranger to
    // the project from then on, while b's grant stays and reaches nothing.
    hub.team("t", &[("a", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    crate::projects::follow(&app).await.unwrap();
    assert_eq!(access(&base, "b", &t).await.0, 404);
    let (st, v) = access(&base, "x", &t).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(powers(&v), [("bobs-box".to_owned(), "b".to_owned(), THREADS_POST.to_owned(), false)], "{v}");
    assert_eq!(v["powers"]["grants"][0]["owner_role"], Value::Null);
    assert_eq!(roles(&v).len(), 2);
}

/// An agent grant token's powers over the project are listed with whose
/// the token is, by its name, never its secret or hash; a revoked token's
/// aren't. A personal project reads as its one owner, and nobody else's.
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
    let (st, v) = access(&base, "x", &t).await;
    assert_eq!(st, 200, "{v}");
    let g = &v["powers"]["grants"];
    assert_eq!(g.as_array().unwrap().len(), 1, "{v}");
    assert_eq!((&g[0]["subject"]["kind"], &g[0]["subject"]["id"]), (&json!("agent"), &json!("agt_live")));
    assert_eq!(g[0]["subject"]["name"], "ci (MCP)");
    assert_eq!(
        (&g[0]["owner"]["account"], &g[0]["power"], &g[0]["reaches"]),
        (&json!("b"), &json!(BOARDS_WRITE), &json!(true))
    );
    assert!(!v.to_string().contains("hash-of"), "{v}");

    // a's personal project: a alone, as its owner; c can't read it.
    let personal = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    let (st, v) = access(&base, "a", &personal).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["project"]["owner"], json!({ "kind": "account", "id": "a" }));
    assert_eq!(roles(&v), [("a".to_owned(), "owner".to_owned())]);
    assert!(v["roles"]["says"].as_str().unwrap().contains("own project"), "{v}");
    assert_eq!(powers(&v), []);
    assert_eq!(access(&base, "c", &personal).await.0, 404);
    assert_eq!(access(&base, "b", &personal).await.0, 404, "a teammate isn't in a's personal project");
    // `personal` names the caller's own.
    let (st, mine) = access(&base, "c", "personal").await;
    assert_eq!(st, 200, "{mine}");
    assert_eq!(roles(&mine), [("c".to_owned(), "owner".to_owned())]);
}

/// What an owner allowed: the delegation as control says it, and the
/// standing run grant with its expiry and who confirmed it (never its
/// bytes or assertion); and the project's cloud workspaces, each saying
/// that an editor has full use.
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
    assert_eq!((&rg["id"], &rg["account"], &rg["expires"]), (&json!(g.id), &json!("a"), &json!(now + 86_400_000)));
    assert_eq!(rg.as_object().unwrap().keys().collect::<Vec<_>>(), ["account", "created_at", "expires", "id", "made"]);
    let ws = &v["workspaces"]["list"][0];
    assert_eq!((&ws["name"], &ws["created_by"]["account"]), (&json!("dev"), &json!("b")));
    assert!(ws["full_use"].as_str().unwrap().contains("editors have full use"), "{v}");
    assert!(v["workspaces"]["says"].as_str().unwrap().contains("checks it itself"), "{v}");
    assert!(v["allow"]["says"].as_str().unwrap().contains("passkey"), "{v}");
    assert!(!v.to_string().contains("\"sig\"") && !v.to_string().contains("\"body\""), "{v}");

    hub.set_delegation(&t, None);
    let (_, v) = access(&base, "a", &t).await;
    assert_eq!(v["allow"]["delegation"], json!({ "live": false, "id": null }));
}
