//! A hosted workspace's setup (M183), through the platform's routes, its
//! worker and `PlatformPort`, against the fake substrate and the test hub:
//! the plan a box reads, its status, a spec change showing "rebuild
//! needed", and a rebuild clearing both. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_control_wire::hosted::{SetupReport, SetupStatus};
use arugula_e2e::now_ms;
use serde_json::json;

use super::{
    EnrollBy,
    tests::{drive, make, serve, show},
};
use crate::{PlatformPort, chat_tests::call};

fn report(status: SetupStatus, step: &str, reason: &str) -> SetupReport {
    SetupReport { status, step: step.into(), reason: reason.into(), pane: Some(4), spec_hash: String::new() }
}

/// A box reads its plan: its repos (cloned with the workspace's GIT_TOKEN,
/// as it has one; a GitHub repo not linked to the project isn't the App's),
/// its packages, its setup script, and the env its spec takes from its
/// secrets, those set and those not. Only its own machine reads it, and
/// says how it goes; members see that on the workspace, and a failure's
/// step and reason code (never the pane's text). A spec edit shows "rebuild needed"; a rebuild clears both.
#[tokio::test]
async fn setup_plan_status_and_rebuild_needed() {
    let (hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let path = format!("/api/projects/{project}/workspaces/{id}");

    // Secrets: an editor sets them, a viewer reads their names, never values.
    let spec = json!({
        "repos": ["https://github.com/acme/app", "https://git.example/acme/lib.git"],
        "packages": ["ripgrep"],
        "setup": "make",
        "env": ["NPM_TOKEN", "NOT_SET"],
    });
    let (status, w) = call(&base, "bob", "PUT", &format!("{path}/spec"), Some(spec)).await;
    assert_eq!(status, 200, "{w}");
    for (name, value) in [("GIT_TOKEN", "tok-1"), ("GIT_USERNAME", "builder"), ("NPM_TOKEN", "npm-1")] {
        let (status, body) =
            call(&base, "bob", "PUT", &format!("{path}/secrets/{name}"), Some(json!({ "value": value }))).await;
        assert_eq!(status, 204, "{name}: {body}");
    }
    assert_eq!(
        call(&base, "vic", "PUT", &format!("{path}/secrets/X"), Some(json!({ "value": "v" }))).await.0,
        403,
        "a viewer sets no secrets"
    );
    assert_eq!(
        call(&base, "bob", "PUT", &format!("{path}/secrets/bad-name"), Some(json!({ "value": "v" }))).await.0,
        400
    );
    let (status, listed) = call(&base, "vic", "GET", &format!("{path}/secrets"), None).await;
    assert_eq!(status, 200);
    let names: Vec<&str> = listed["secrets"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["GIT_TOKEN", "GIT_USERNAME", "NPM_TOKEN"]);
    assert!(!listed.to_string().contains("tok-1"), "{listed}");
    let sealed = app.db.hosted_secret_sealed(&id, "GIT_TOKEN").unwrap().unwrap();
    assert!(sealed.key_version > crate::keys::MASTER, "sealed with the project's key (M209)");

    let now = now_ms();
    drive(&app, now).await;
    // Before the worker sees it enroll, any machine for the box may read
    // it (the hub checked its certificate names this workspace).
    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().expect("its box");
    hub.enroll(&id, "hosted-machine-1");
    drive(&app, now + 3_000).await;
    assert_eq!(show(&base, &project, &id).await["state"], "ready");

    assert_eq!(plan.workspace, id);
    let repos: Vec<_> = plan
        .repos
        .iter()
        .map(|r| (r.url.as_str(), r.via.as_str(), r.username.as_deref(), r.password.as_deref()))
        .collect();
    assert_eq!(
        repos,
        [
            ("https://github.com/acme/app", "secret", Some("builder"), Some("tok-1")),
            ("https://git.example/acme/lib.git", "secret", Some("builder"), Some("tok-1")),
        ]
    );
    assert_eq!(plan.packages, ["ripgrep"]);
    assert_eq!(plan.setup, "make");
    assert_eq!(plan.env.get("NPM_TOKEN").map(String::as_str), Some("npm-1"));
    assert_eq!(plan.unset, ["NOT_SET"]);
    assert_eq!(show(&base, &project, &id).await["rebuild_needed"], false, "built from the spec it read");

    // Another machine (an old box, or anyone else's) reads nothing and says
    // nothing, and neither does a box of another workspace or project.
    assert!(app.hosted_plan(&project, &id, "someone-else").await.unwrap().is_none());
    assert!(app.hosted_plan("prj_other", &id, "hosted-machine-1").await.unwrap().is_none());
    let r = report(SetupStatus::SettingUp, "clone app", "");
    assert!(!app.hosted_setup(&project, &id, "someone-else", &r).await.unwrap());

    // Its own machine says how it goes.
    assert!(app.hosted_setup(&project, &id, "hosted-machine-1", &r).await.unwrap());
    let w = show(&base, &project, &id).await;
    assert_eq!(w["setup"]["status"], "setting_up");
    assert_eq!(w["setup"]["step"], "clone app");
    assert_eq!(w["setup"]["pane"], 4);
    assert!(w["setup"].get("reason").is_none(), "no reason while it runs: {w}");
    let r = report(SetupStatus::Failed, "setup script", "exit");
    assert!(app.hosted_setup(&project, &id, "hosted-machine-1", &r).await.unwrap());
    let w = show(&base, &project, &id).await;
    let setup = &w["setup"];
    assert_eq!(
        (setup["status"].as_str(), setup["step"].as_str(), setup["reason"].as_str()),
        (Some("failed"), Some("setup script"), Some("exit"))
    );
    assert!(setup.get("tail").is_none(), "no pane text on the platform: {w}");
    // Anything but a known code is dropped.
    let r = report(SetupStatus::Failed, "setup script", "make: *** [all] Error 2");
    assert!(app.hosted_setup(&project, &id, "hosted-machine-1", &r).await.unwrap());
    assert!(show(&base, &project, &id).await["setup"].get("reason").is_none());
    let (_, listed) = call(&base, "vic", "GET", &format!("/api/projects/{project}/workspaces"), None).await;
    assert_eq!(listed["workspaces"][0]["setup"]["status"], "failed", "members see it in the list");

    // A spec edit: rebuild needed, the box untouched.
    let spec = json!({ "repos": ["https://git.example/acme/lib.git"], "setup": "make test" });
    assert_eq!(call(&base, "vic", "PUT", &format!("{path}/spec"), Some(spec.clone())).await.0, 403);
    let bad = json!({ "repos": ["http://git.example/acme/lib.git"] });
    assert_eq!(call(&base, "bob", "PUT", &format!("{path}/spec"), Some(bad)).await.0, 422);
    let (status, w) = call(&base, "bob", "PUT", &format!("{path}/spec"), Some(spec)).await;
    assert_eq!(status, 200, "{w}");
    assert_eq!((w["rebuild_needed"].as_bool(), w["state"].as_str()), (Some(true), Some("ready")));
    assert_eq!(w["spec"]["setup"], "make test");
    assert_eq!(w["setup"]["status"], "failed", "what the box said stays until it's rebuilt");

    // A rebuild: a new box, which hasn't said anything yet, built from the
    // spec it reads.
    assert_eq!(call(&base, "bob", "POST", &format!("{path}/rebuild"), None).await.0, 200);
    drive(&app, now + 6_000).await;
    let w = show(&base, &project, &id).await;
    assert!(w["setup"].is_null(), "{w}");
    assert_eq!(w["rebuild_needed"], false);
    hub.enroll(&id, "hosted-machine-2");
    drive(&app, now + 9_000).await;
    let plan = app.hosted_plan(&project, &id, "hosted-machine-2").await.unwrap().expect("the new box");
    assert_eq!(plan.setup, "make test");
    assert!(app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().is_none(), "the old box");

    // Removing a secret: the next plan clones with nothing.
    assert_eq!(call(&base, "bob", "DELETE", &format!("{path}/secrets/GIT_TOKEN"), None).await.0, 204);
    assert_eq!(call(&base, "bob", "DELETE", &format!("{path}/secrets/GIT_TOKEN"), None).await.0, 404);
    let plan = app.hosted_plan(&project, &id, "hosted-machine-2").await.unwrap().unwrap();
    assert_eq!((plan.repos[0].via.as_str(), plan.repos[0].password.as_deref()), ("none", None));
}

/// The hub asks an out-of-process platform for both over the internal API.
#[tokio::test]
async fn setup_over_the_internal_api() {
    let (hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    drive(&app, now_ms()).await;

    let key = arugula_control_common::principal::PrincipalKey::new(&[7; 32]).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let router = crate::remote::serve_platform(app.clone() as Arc<dyn PlatformPort>, key.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let remote = crate::remote::HttpPlatform::new(&url, key).unwrap();

    let plan = remote.hosted_plan(&project, &id, "m1").await.unwrap().expect("its box");
    assert_eq!(plan.repos[0].url, "https://github.com/acme/app");
    assert!(remote.hosted_plan(&project, "hw_nope", "m1").await.unwrap().is_none());
    assert!(remote.hosted_setup(&project, &id, "m1", &report(SetupStatus::Ready, "setup script", "")).await.unwrap());
    assert_eq!(show(&base, &project, &id).await["setup"]["status"], "ready");
}

/// A GitHub repo linked to the project: the plan gives each box a token of
/// its own for it, its contents read-only, never the platform's kept one,
/// with where the box revokes it, logged under the machine. Asked again,
/// it's the same token, minted once; once the box says it's past its
/// clones, there's none. Unlinking revokes it; the box then clones with
/// the workspace's GIT_TOKEN, as a repo never linked does, and gets the
/// App's again once it's linked again.
#[tokio::test]
async fn a_box_clones_a_linked_repo_with_a_token_of_its_own() {
    use crate::{
        db::forge::{Installation, LinkedRepo},
        forge::{
            Caller, GithubApp, Purpose, Repo,
            links_tests::{Shared, fake_github, throwaway_key},
        },
    };
    let gh: Shared = Arc::default();
    {
        let mut g = gh.lock().unwrap();
        g.numbered = true;
        g.installed.insert("acme/app".into(), (9, 500, "acme".into()));
        g.repos.insert("acme/app".into(), (77, true));
    }
    let api = fake_github(gh.clone()).await;
    let fake = Arc::new(super::tests::Fake::default());
    let (hub, app, base, project) = super::tests::serve_platform(EnrollBy::Sandbox, fake, "tpl", |p| {
        p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), "s".into(), &api, throwaway_key()));
    })
    .await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let path = format!("/api/projects/{project}/workspaces/{id}");
    let spec = json!({ "repos": ["https://github.com/acme/app", "https://github.com/acme/other"] });
    assert_eq!(call(&base, "bob", "PUT", &format!("{path}/spec"), Some(spec)).await.0, 200);
    let put = Some(json!({ "value": "tok-1" }));
    assert_eq!(call(&base, "bob", "PUT", &format!("{path}/secrets/GIT_TOKEN"), put).await.0, 204);
    let now = now_ms();
    let repo = LinkedRepo {
        repo_id: 77,
        full_name: "acme/app".into(),
        private: true,
        project_id: project.clone(),
        installation_id: 9,
        linked_by: Some("alice".into()),
        linked_at: now,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let install = Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    app.db.link_repo(&repo, &install, "alice", now).unwrap();
    // The platform's own read token for it, kept.
    let github = app.github.as_ref().unwrap();
    let kept = github.read_token(&Caller::of(&app, "platform"), 9, Repo::Id(77), Purpose::Ingest).await.unwrap();
    let kept = kept.secret().to_owned();
    drive(&app, now).await;

    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().expect("its box");
    let first = &plan.repos[0];
    assert_eq!((first.via.as_str(), first.username.as_deref()), ("github_app", Some("x-access-token")));
    let token = first.password.clone().unwrap();
    assert_ne!(token, kept, "not the platform's kept token");
    assert_eq!(first.revoke.as_deref(), Some(format!("{api}/installation/token").as_str()));
    assert_eq!(
        gh.lock().unwrap().minted.last().unwrap(),
        &json!({ "metadata": "read", "contents": "read" }),
        "its contents, read, and nothing else"
    );
    let other = &plan.repos[1];
    assert_eq!(
        (other.via.as_str(), other.password.as_deref(), other.revoke.as_deref()),
        ("secret", Some("tok-1"), None)
    );

    // Asked again and again: the same token, minted once.
    let minted = gh.lock().unwrap().minted.len();
    for _ in 0..20 {
        let again = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().unwrap();
        assert_eq!(again.repos[0].password.as_deref(), Some(token.as_str()));
    }
    assert_eq!(gh.lock().unwrap().minted.len(), minted, "no mint for a repeat");
    // Another machine (a rebuild's) gets its own.
    let theirs = app.hosted_plan(&project, &id, "hosted-machine-2").await.unwrap().unwrap();
    let theirs = theirs.repos[0].password.clone().unwrap();
    assert_ne!(theirs, token);
    let kept_now = github.read_token(&Caller::of(&app, "platform"), 9, Repo::Id(77), Purpose::Ingest).await.unwrap();
    assert_eq!(kept_now.secret(), kept, "the platform's own stays its own");
    // Logged under the machine that asked.
    let callers: Vec<String> =
        app.db.token_mints(0).unwrap().into_iter().filter(|m| m.purpose == "hosted:clone").map(|m| m.caller).collect();
    assert_eq!(callers, ["hosted-machine-1", "hosted-machine-2"]);

    // Unlinked: the boxes' tokens are revoked, and the box clones with the
    // workspace's GIT_TOKEN, as before the App.
    let (s, v) = call(&base, "alice", "DELETE", &format!("/api/projects/{project}/repos/acme/app"), None).await;
    assert_eq!(s, 200, "{v}");
    let revoked = gh.lock().unwrap().revokes.clone();
    assert!(revoked.contains(&token) && revoked.contains(&theirs) && revoked.contains(&kept), "{revoked:?}");
    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().unwrap();
    let first = &plan.repos[0];
    assert_eq!(
        (first.via.as_str(), first.password.as_deref(), first.revoke.as_deref()),
        ("secret", Some("tok-1"), None)
    );

    // Linked again: the App's token again, a new one.
    app.db.link_repo(&repo, &install, "alice", now_ms()).unwrap();
    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().unwrap();
    assert_eq!(plan.repos[0].via, "github_app");
    let relinked = plan.repos[0].password.clone().unwrap();
    assert!(relinked != token && relinked != theirs, "a new one");

    // The box says it's past its clones: no App token any more.
    let r = report(SetupStatus::SettingUp, "packages", "");
    assert!(app.hosted_setup(&project, &id, "hosted-machine-1", &r).await.unwrap());
    let minted = gh.lock().unwrap().minted.len();
    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().unwrap();
    assert_eq!((plan.repos[0].via.as_str(), plan.repos[0].revoke.as_deref()), ("secret", None));
    assert_eq!(gh.lock().unwrap().minted.len(), minted);
    // Nor once it ended, even at a clone.
    let r = report(SetupStatus::Failed, "clone app", "exit");
    assert!(app.hosted_setup(&project, &id, "hosted-machine-1", &r).await.unwrap());
    let plan = app.hosted_plan(&project, &id, "hosted-machine-1").await.unwrap().unwrap();
    assert_eq!(plan.repos[0].via, "secret");
}
