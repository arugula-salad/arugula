//! A deploy's title and the version a release expects (#333).

use arugula_e2e::drivers::arugula_release::Lane;

use super::titles::{Facts, SUBJECT_MAX, expected_version, fetch, subject_of, title};
use crate::db::deploys::{Environment, Source, SourceKind};

fn env(driver: &str) -> Environment {
    Environment {
        id: "env_1".into(),
        project: "p".into(),
        name: "production".into(),
        driver: driver.into(),
        secret_names: vec![],
        settings: Default::default(),
        created_at: 1,
        updated_at: 1,
    }
}

fn commit() -> Source {
    Source {
        kind: SourceKind::Commit,
        reference: "b1faaa04c0ffee00".repeat(2).chars().take(40).collect(),
        place: "a/b".into(),
    }
}

#[test]
fn a_title_says_what_is_deployed_and_never_a_certain_version() {
    let (rel, other) = (env("arugula-release"), env("chant-fly"));
    let t = |e, s, v| title(e, &commit(), s, v);
    assert_eq!(
        t(&rel, Some("Docs: the Arugula look"), Some("0.63.9")),
        "Release 0.63.9 (expected) · b1faaa04 · Docs: the Arugula look"
    );
    assert_eq!(t(&rel, None, Some("0.63.9")), "Release 0.63.9 (expected) · b1faaa04");
    // A release whose version wasn't read is a deploy like another.
    assert_eq!(t(&rel, Some("Fix"), None), "Deploy b1faaa04 to production · Fix");
    assert_eq!(t(&other, Some("Fix"), Some("9.9.9")), "Deploy b1faaa04 to production · Fix");
    assert_eq!(t(&other, None, None), "Deploy to production");
}

#[test]
fn the_expected_version_is_what_release_bump_makes() {
    let toml =
        |v: &str| format!("[package]\nname = \"x\"\nversion = \"9.9.9\"\n\n[workspace.package]\nversion = \"{v}\"\n");
    // The higher of the commit's and main's, then one bump up.
    assert_eq!(expected_version(Lane::Daemon, &toml("0.63.8"), &toml("0.63.9"), "patch").as_deref(), Some("0.63.10"));
    assert_eq!(expected_version(Lane::Daemon, &toml("0.64.0"), &toml("0.63.9"), "minor").as_deref(), Some("0.65.0"));
    assert_eq!(expected_version(Lane::Daemon, &toml("0.63.8"), &toml("0.63.9"), "major").as_deref(), Some("1.0.0"));
    // The app's lane reads the desktop crate's [package].
    assert_eq!(expected_version(Lane::App, &toml("0.1.0"), &toml("0.1.0"), "patch").as_deref(), Some("9.9.10"));
    assert_eq!(expected_version(Lane::Daemon, "no version", &toml("0.63.9"), "patch"), None);
}

/// A ref that isn't a commit id never reaches a URL: no fetch (this app has
/// no GitHub, so a fetch would be an error, not the defaults).
#[tokio::test]
async fn a_ref_that_is_not_a_commit_id_is_not_fetched() {
    let app = crate::Platform::new(
        crate::Config::default(),
        crate::db::Db::test(),
        std::sync::Arc::new(crate::chat_tests::TestHub::default()),
        reqwest::Client::new(),
        Default::default(),
    );
    for bad in ["dead", "../../x", &"g".repeat(40)] {
        let src = Source { kind: SourceKind::Commit, reference: bad.into(), place: "a/b".into() };
        assert_eq!(fetch(&app, &env("chant-fly"), &src).await.unwrap(), Facts::default(), "{bad}");
    }
}

#[test]
fn a_subject_is_the_first_line_cut_short() {
    assert_eq!(subject_of("  Docs: look \n\nbody").as_deref(), Some("Docs: look"));
    assert_eq!(subject_of("\n").as_deref(), None);
    let long = subject_of(&"w".repeat(300)).unwrap();
    assert_eq!(long.chars().count(), SUBJECT_MAX);
    assert!(long.ends_with('…'));
}

mod released {
    //! A release of a commit that's in a release already is refused when
    //! it's planned (#334), through a fake GitHub.

    use std::sync::{Arc, Mutex};

    use aws_lc_rs::{encoding::AsDer, rsa::KeySize};
    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::{commit, env};
    use crate::{Platform, db::Db, drivers::titles::already_released, forge::GithubApp};

    /// What the fake says: the releases, and how the compare of the commit
    /// with the lane's newest tag came out (`None`: GitHub is down).
    struct Gh {
        releases: Vec<&'static str>,
        compare: Option<&'static str>,
        compared: Vec<String>,
    }

    type Shared = Arc<Mutex<Gh>>;

    async fn releases(State(h): State<Shared>) -> Response {
        if h.lock().unwrap().compare.is_none() {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        let v: Vec<Value> =
            h.lock().unwrap().releases.iter().map(|t| json!({ "tag_name": t, "draft": false })).collect();
        Json(v).into_response()
    }

    async fn compare(State(h): State<Shared>, Path((_, _, spec)): Path<(String, String, String)>) -> Response {
        let mut h = h.lock().unwrap();
        h.compared.push(spec);
        Json(json!({ "status": h.compare })).into_response()
    }

    async fn platform(gh: Shared) -> Platform {
        let r = Router::new()
            .route(
                "/repos/{o}/{r}/installation",
                get(async || {
                    Json(json!({ "id": 9, "account": { "id": 5, "login": "arugula-salad", "type": "Organization" } }))
                }),
            )
            .route(
                "/app/installations/{id}/access_tokens",
                post(async || {
                    Json(json!({
                        "token": "ghs_fake", "expires_at": "2099-01-01T00:00:00Z",
                        "repositories": [{ "id": 1, "full_name": "arugula-salad/arugula" }],
                    }))
                }),
            )
            .route("/repos/{o}/{r}/releases", get(releases))
            .route("/repos/{o}/{r}/compare/{spec}", get(compare))
            .with_state(gh);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
        let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
        let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
        let key = aws_lc_rs::rsa::KeyPair::from_pkcs8(der.as_ref()).unwrap();
        let mut p = Platform::new(
            crate::Config::default(),
            Db::test(),
            Arc::new(crate::chat_tests::TestHub::default()),
            reqwest::Client::new(),
            Default::default(),
        );
        p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), "whsec-test".into(), &api, key));
        p
    }

    #[tokio::test]
    async fn a_commit_in_the_lanes_newest_release_is_refused_naming_its_tag() {
        // The newest `v` tag, not the app's newer one; the commit is behind it.
        let gh: Shared = Arc::new(Mutex::new(Gh {
            releases: vec!["app-v0.2.0", "v0.63.8", "v0.63.7"],
            compare: Some("behind"),
            compared: vec![],
        }));
        let app = platform(gh.clone()).await;
        let (rel, src) = (env("arugula-release"), commit());
        let said = already_released(&app, &rel, &src).await;
        assert_eq!(said.as_deref(), Some("b1faaa04 is already in v0.63.8"));
        assert_eq!(gh.lock().unwrap().compared, [format!("v0.63.8...{}", src.reference)]);

        // Ahead of the tag (not released), and another driver: nothing to say.
        gh.lock().unwrap().compare = Some("ahead");
        assert_eq!(already_released(&app, &rel, &src).await, None);
        gh.lock().unwrap().compare = Some("behind");
        assert_eq!(already_released(&app, &env("chant-fly"), &src).await, None);
    }

    #[tokio::test]
    async fn a_github_failure_lets_the_plan_through() {
        let gh: Shared = Arc::new(Mutex::new(Gh { releases: vec!["v0.63.8"], compare: None, compared: vec![] }));
        let app = platform(gh).await;
        assert_eq!(already_released(&app, &env("arugula-release"), &commit()).await, None);
    }
}
