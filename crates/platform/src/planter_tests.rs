//! planter's import (M232): its backup as planter writes it, and its repos
//! linked to projects against a fake GitHub and a test hub, run twice.
//! Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::*;
use crate::{
    chat_tests::TestHub,
    db::sql::{Sql as _, params},
    forge::{
        GithubApp,
        links_tests::{SECRET, Shared, fake_github, throwaway_key},
    },
};

/// The made-up backup (`fixtures/planter-backup`).
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/planter-backup")
}

/// Every file in a directory and its bytes.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name().into_string().unwrap(), std::fs::read(e.path()).unwrap())
        })
        .collect();
    out.sort();
    out
}

/// A backup of our own, in a fresh directory.
fn scratch(accounts: Value, boxes: Option<Value>) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("planter-backup-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("accounts.json"), accounts.to_string()).unwrap();
    if let Some(b) = boxes {
        std::fs::write(dir.join("boxes.json"), b.to_string()).unwrap();
    }
    dir
}

#[test]
fn the_backup_reads_as_planter_writes_it() {
    let b = Backup::read(&fixture()).unwrap();
    let ids: Vec<(u64, &str)> = b.accounts.iter().map(|a| (a.github_id, a.login.as_str())).collect();
    assert_eq!(ids, [(1111, "jake-gh"), (4242, "jake-gh"), (5151, "alice"), (7777, "newbie")]);
    assert!(b.accounts.iter().all(|a| a.platform_account.is_none()), "null and missing are both unlinked");
    assert_eq!(b.skipped.len(), 1);
    assert_eq!((b.skipped[0].key.as_str(), b.skipped[0].why.as_str()), ("ghost", "no numeric GitHub id"));
    let boxes: Vec<&str> = b.boxes.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(boxes, ["fern-a1b2", "fern-c3d4", "fern-e5f6", "fern-g7h8", "fern-i9j0", "fern-k1l2"]);
    assert_eq!(b.boxes[0].owner, "4242");
    // Once per person and repo, any case, in the order of their first box.
    let planted = b.planted();
    let planted: Vec<(&str, &str)> = planted.iter().map(|(o, r)| (o.as_str(), r.as_str())).collect();
    assert_eq!(
        planted,
        [
            ("4242", "jake-gh/kit-app"),
            ("4242", "acme/site"),
            ("5151", "jake-gh/kit-app"),
            ("7777", "newbie/thing"),
            ("4242", "jake-gh/no-app"),
        ]
    );
    // Nothing secret is carried: no box's sealed secrets or handles.
    let shown = serde_json::to_string(&b).unwrap();
    assert!(!shown.contains("sealed-") && !shown.contains("env_fixture"), "{shown}");

    // A directory that isn't a backup says so; a key that isn't its id is
    // skipped; a missing boxes.json is no boxes.
    let e = Backup::read(&std::env::temp_dir().join("no-such-planter-backup")).unwrap_err();
    assert!(format!("{e:#}").contains("no accounts.json"), "{e:#}");
    let dir = scratch(json!({ "12": { "id": 13, "login": "odd" }, "14": { "id": "14", "login": "str" } }), None);
    let b = Backup::read(&dir).unwrap();
    assert_eq!(b.accounts.iter().map(|a| a.github_id).collect::<Vec<_>>(), [14]);
    assert_eq!(b.skipped[0].why, "its key isn't its GitHub id");
    assert!(b.boxes.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn org_targets_parse() {
    assert_eq!(parse_org("Acme=prj_1").unwrap(), ("acme".into(), OrgTarget::Project("prj_1".into())));
    assert_eq!(parse_org("acme=personal").unwrap(), ("acme".into(), OrgTarget::Personal));
    assert!(parse_org("acme").is_err() && parse_org("=prj").is_err() && parse_org("acme=").is_err());
}

/// The platform over `hub` with the App, against a fake GitHub where:
/// `jake-gh/kit-app` (101) is installed on the user jake-gh, its admin
/// (4242), with alice (5151) a maintainer; `acme/site` (202, private) on the
/// organization acme, jake-gh its admin; `jake-gh/no-app` isn't installed.
/// jake and alice signed in with GitHub and are linked as planter's users;
/// newbie (7777) isn't on Arugula.
async fn world() -> (Arc<TestHub>, Shared, Platform) {
    let hub = Arc::new(TestHub::default());
    let gh: Shared = Arc::default();
    let api = fake_github(gh.clone()).await;
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("jake-gh/kit-app".into(), (9, 4242, "jake-gh".into()));
        g.user_accounts.insert("jake-gh".into());
        g.repos.insert("jake-gh/kit-app".into(), (101, false));
        g.roles.insert(("jake-gh/kit-app".into(), "jake-gh".into()), ("admin".into(), 4242));
        g.roles.insert(("jake-gh/kit-app".into(), "alice".into()), ("maintain".into(), 5151));
        g.installed.insert("acme/site".into(), (10, 500, "acme".into()));
        g.repos.insert("acme/site".into(), (202, true));
        g.roles.insert(("acme/site".into(), "jake-gh".into()), ("admin".into(), 4242));
    }
    hub.github("acct-jake", 4242, "jake-gh");
    hub.github("acct-alice", 5151, "alice");
    hub.link_product(CLIENT, "4242", "acct-jake", 4242);
    hub.link_product(CLIENT, "5151", "acct-alice", 5151);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), &api, throwaway_key()));
    (hub, gh, p)
}

/// What the import writes, as rows: linked repos, its record and projects.
fn rows(p: &Platform) -> Vec<String> {
    let c = p.db.c();
    let mut out = Vec::new();
    for (t, cols) in [
        ("linked_repos", "repo_id, full_name, project_id, installation_id, linked_by, unlinked_at"),
        ("planter_imports", "planter_user, repo, repo_id, project_id, account_id, outcome, imported_at"),
        ("projects", "id, account_id, team_id, name"),
    ] {
        let cast: Vec<String> = cols.split(", ").map(|c| format!("COALESCE(CAST({c} AS TEXT), 'NULL')")).collect();
        let sql = format!("SELECT {} FROM {t} ORDER BY 1, 2", cast.join(", "));
        let n = cast.len();
        let got = c
            .query_map(&sql, params![], |r| {
                let mut v = Vec::new();
                for i in 0..n {
                    v.push(r.get::<String>(i)?);
                }
                Ok(format!("{t}: {}", v.join(" | ")))
            })
            .unwrap();
        out.extend(got);
    }
    out
}

fn outcomes(r: &Report) -> Vec<(&str, &str, &Outcome)> {
    r.repos.iter().map(|l| (l.owner.as_str(), l.repo.as_str(), &l.outcome)).collect()
}

#[tokio::test]
async fn repos_link_by_planter_and_a_second_run_changes_nothing() {
    let (hub, gh, p) = world().await;
    let team = p.db.team_project("t-acme", "Acme", 1).unwrap();
    hub.team("t-acme", &[("acct-jake", TeamRole::Editor), ("acct-alice", TeamRole::Viewer)]);
    let backup = Backup::read(&fixture()).unwrap();
    let before = files(&fixture());
    let empty = rows(&p);

    // Dry, with no say about acme: what it would do, and nothing written.
    let mut o = Options { client: CLIENT.into(), orgs: BTreeMap::new(), dry_run: true };
    let r = import_repos(&p, &backup, &o).await.unwrap();
    let jake_personal = "(the personal project of acct-jake, made now)".to_owned();
    assert_eq!(
        outcomes(&r),
        [
            (
                "4242",
                "jake-gh/kit-app",
                &Outcome::Linked { project: jake_personal.clone(), repo_id: 101, personal: true, made: true }
            ),
            ("4242", "acme/site", &Outcome::WaitingOrg { org: "acme".into() }),
            ("5151", "jake-gh/kit-app", &Outcome::Elsewhere { project: jake_personal }),
            ("7777", "newbie/thing", &Outcome::WaitingSignIn),
            (
                "4242",
                "jake-gh/no-app",
                &Outcome::WaitingApp {
                    install_url:
                        "https://github.com/apps/arugula-test/installations/new?state=personal%3Ajake-gh%2Fno-app"
                            .into()
                }
            ),
        ]
    );
    assert_eq!((r.changed(), r.waiting()), (1, 3));
    assert_eq!(rows(&p), empty, "a dry run writes nothing");
    let text = r.render(&fixture());
    assert!(text.contains("DRY RUN") && text.contains("would link") && text.contains("nothing written"), "{text}");
    // Each box is reported, with its owner, and nothing of it imported.
    assert_eq!(r.boxes.len(), 6);
    assert_eq!((r.boxes[0].login.as_str(), r.boxes[0].account.as_deref()), ("jake-gh", Some("acct-jake")));
    assert_eq!((r.boxes[3].login.as_str(), r.boxes[3].account.as_deref()), ("newbie", None));
    assert!(text.contains("nothing imported") && text.contains("fern-g7h8"), "{text}");

    // For real, with acme's repos on its team's project.
    o.dry_run = false;
    o.orgs.insert("acme".into(), OrgTarget::Project(team.clone()));
    let r = import_repos(&p, &backup, &o).await.unwrap();
    let personal = p.db.personal_project_of("acct-jake").unwrap().expect("made now");
    assert_eq!(
        outcomes(&r)[..3],
        [
            (
                "4242",
                "jake-gh/kit-app",
                &Outcome::Linked { project: personal.clone(), repo_id: 101, personal: true, made: true }
            ),
            (
                "4242",
                "acme/site",
                &Outcome::Linked { project: team.clone(), repo_id: 202, personal: false, made: false }
            ),
            ("5151", "jake-gh/kit-app", &Outcome::Elsewhere { project: personal.clone() }),
        ]
    );
    assert_eq!((r.changed(), r.waiting()), (2, 2));
    let kit = p.db.linked_repo(101).unwrap().unwrap();
    assert_eq!(
        (kit.project_id.as_str(), kit.linked_by.as_deref(), kit.installation_id),
        (personal.as_str(), Some("acct-jake"), 9)
    );
    let site = p.db.linked_repo(202).unwrap().unwrap();
    assert_eq!((site.project_id.as_str(), site.private), (team.as_str(), true));
    assert_eq!(p.db.planter_imported("4242", "Jake-GH/Kit-App").unwrap().map(|x| x.0), Some(personal.clone()));
    assert!(p.db.personal_project_of("acct-alice").unwrap().is_none(), "nothing made for a repo not linked");
    let done = rows(&p);

    // Again: nothing changes, and GitHub isn't asked about what's done.
    let asked = gh.lock().unwrap().asked;
    let r = import_repos(&p, &backup, &o).await.unwrap();
    assert_eq!(r.changed(), 0);
    assert!(matches!(r.repos[0].outcome, Outcome::ImportedBefore { ref project, .. } if *project == personal));
    assert!(matches!(r.repos[1].outcome, Outcome::ImportedBefore { ref project, .. } if *project == team));
    assert_eq!(rows(&p), done);
    assert_eq!(gh.lock().unwrap().asked, asked + 1, "only alice's role is asked again");

    // A person's unlink sticks, through the purge.
    p.db.unlink_repo(&personal, "jake-gh/kit-app", arugula_e2e::now_ms()).unwrap().unwrap();
    while p.db.purge_one(u64::MAX / 2).unwrap().is_some() {}
    assert!(p.db.linked_repo(101).unwrap().is_none());
    let r = import_repos(&p, &backup, &o).await.unwrap();
    assert!(matches!(r.repos[0].outcome, Outcome::ImportedBefore { .. }));
    // Nor for alice, who maintains it too: the import linked it once.
    assert!(matches!(r.repos[2].outcome, Outcome::ImportedBefore { ref project, .. } if *project == personal));
    assert!(p.db.linked_repo(101).unwrap().is_none(), "never linked again by the import");
    assert!(p.db.personal_project_of("acct-alice").unwrap().is_none());

    // Someone who wasn't on Arugula is picked up by a later run.
    hub.github("acct-new", 7777, "newbie");
    hub.link_product(CLIENT, "7777", "acct-new", 7777);
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("newbie/thing".into(), (11, 7777, "newbie".into()));
        g.user_accounts.insert("newbie".into());
        g.repos.insert("newbie/thing".into(), (303, false));
        g.roles.insert(("newbie/thing".into(), "newbie".into()), ("admin".into(), 7777));
    }
    let r = import_repos(&p, &backup, &o).await.unwrap();
    assert!(matches!(r.repos[3].outcome, Outcome::Linked { repo_id: 303, .. }), "{:?}", r.repos[3]);
    assert_eq!(r.changed(), 1);
    assert_eq!(files(&fixture()), before, "the backup is only read");
}

#[tokio::test]
async fn a_planter_links_only_what_they_may() {
    let (hub, gh, p) = world().await;
    let team = p.db.team_project("t-acme", "Acme", 1).unwrap();
    // jake only views acme's team project; alice writes kit-app, no more.
    hub.team("t-acme", &[("acct-jake", TeamRole::Viewer)]);
    gh.lock().unwrap().roles.insert(("jake-gh/kit-app".into(), "alice".into()), ("write".into(), 5151));
    // A link the hub has whose GitHub id isn't the planter's is no link.
    hub.link_product(CLIENT, "7777", "acct-jake", 4242);
    let dir = scratch(
        json!({}),
        Some(json!({
            "b1": { "name": "b1", "repo": "acme/site", "owner": "4242", "createdAt": 1 },
            "b2": { "name": "b2", "repo": "jake-gh/kit-app", "owner": "5151", "createdAt": 2 },
            "b3": { "name": "b3", "repo": "newbie/thing", "owner": "7777", "createdAt": 3 },
            "b4": { "name": "b4", "repo": "not a repo", "owner": "4242", "createdAt": 4 },
        })),
    );
    let backup = Backup::read(&dir).unwrap();
    let mut orgs = BTreeMap::new();
    orgs.insert("acme".into(), OrgTarget::Project(team.clone()));
    let o = Options { client: CLIENT.into(), orgs, dry_run: false };
    let r = import_repos(&p, &backup, &o).await.unwrap();
    assert_eq!(
        outcomes(&r),
        [
            ("4242", "acme/site", &Outcome::NotMember { project: team }),
            ("5151", "jake-gh/kit-app", &Outcome::Role { login: "alice".into(), role: "write".into() }),
            ("7777", "newbie/thing", &Outcome::WaitingSignIn),
            ("4242", "not a repo", &Outcome::Refused { why: "\"not a repo\" isn't OWNER/NAME".into() }),
        ]
    );
    assert_eq!(r.changed(), 0);
    assert_eq!(p.db.c().query_row("SELECT count(*) FROM linked_repos", params![], |r| r.get::<i64>(0)).unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn no_app_no_import() {
    let hub = Arc::new(TestHub::default());
    let p = Platform::new(Default::default(), crate::db::Db::test(), hub, reqwest::Client::new(), Default::default());
    let o = Options { client: CLIENT.into(), ..Default::default() };
    let e = import_repos(&p, &Backup::read(&fixture()).unwrap(), &o).await.unwrap_err();
    assert!(format!("{e:#}").contains("GITHUB_APP_ID"), "{e:#}");
}
