//! planter's drivers' plans and settings, and the vendored renderer held to
//! planter's own output, captured from planter (`scripts/vendor-planter`).

use super::*;
use crate::drivers::{plan_text, planned, setting_fields as fields_of, settings as settings_of};

fn s(kv: &[(&str, &str)]) -> Settings {
    kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn digest(n: char) -> String {
    format!("sha256:{}", n.to_string().repeat(64))
}

/// Each vendored file is planter's byte for byte without its `// arugula
/// vendor:` lines: their own digest is the one those lines name, and the
/// lines name the commit `VENDORED_FROM` does.
#[test]
fn the_vendored_files_are_planters_but_for_their_header() {
    for (path, text) in &RENDERER[..2] {
        let header: Vec<&str> = text.lines().filter(|l| l.starts_with("// arugula vendor: ")).collect();
        assert_eq!(header.len(), 6, "{path}");
        assert!(header[0].contains(VENDORED_FROM.split('@').nth(1).unwrap()), "{path}: {}", header[0]);
        let named = header[5].rsplit("sha256 ").next().unwrap().trim_end_matches(").");
        let mut body = String::new();
        for l in text.split_inclusive('\n') {
            if !l.starts_with("// arugula vendor: ") {
                body.push_str(l);
            }
        }
        assert_eq!(hex::encode(sha2::Sha256::digest(body.as_bytes())), named, "{path}");
    }
    assert!(RENDERER[2].1.contains("export async function answer"), "the shim is ours");
}

/// planter's own output for each fixture (its tests' manifests, the real
/// app.yaml of ocarina, beds-smoke and two shipped apps, a ship, a promote
/// and ocarina's scaffold), as `scripts/vendor-planter` captured it from
/// planter, is what the vendored renderer answers, byte for byte. Needs
/// node, as a box does.
#[cfg(unix)]
#[test]
fn the_vendored_renderer_renders_planters_fixtures_identically() {
    use std::{io::Write as _, process::Command};
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node here");
        return;
    }
    let dir = std::env::temp_dir().join(format!("arugula-planter-renderer-{}", std::process::id()));
    for (path, text) in RENDERER {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/planter/fixtures");
    let mut seen = vec![];
    for e in std::fs::read_dir(&fixtures).unwrap() {
        let path = e.unwrap().path();
        let fx: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(fx["captured"]["repo"], "arugula-salad/planter", "{}", path.display());
        let mut c = Command::new("node")
            .arg(dir.join("renderer.mjs"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(fx["request"].to_string().as_bytes()).unwrap();
        let out = c.wait_with_output().unwrap();
        let got: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let mut want = fx["planter"].clone();
        want.as_object_mut().unwrap().remove("committedMatches");
        assert_eq!(got, want, "{}", fx["name"]);
        seen.push(fx["name"].as_str().unwrap().to_owned());
    }
    let _ = std::fs::remove_dir_all(&dir);
    seen.sort();
    for want in ["real-ocarina", "real-redone-hello", "ship-hello", "promote-hello", "scaffold-ocarina", "fern-hosts"] {
        assert!(seen.iter().any(|s| s == want), "{want} in {seen:?}");
    }
}

#[test]
fn settings_are_checked_and_filled_in() {
    assert_eq!(settings(PROD, &s(&[])).unwrap(), s(&[(APP_DIR, "app")]));
    assert!(settings(GO_LIVE, &s(&[])).unwrap().is_empty());
    assert!(settings(GO_LIVE, &s(&[(APP, "x")])).unwrap_err().contains("it takes domains"));
    assert!(settings(GO_LIVE, &s(&[(DOMAINS, "shop.example.com www.example.com")])).is_ok());
    for bad in ["shop", "Shop.example.com", "a.com a.com", "-a.com", "a.com,b.com"] {
        assert!(settings(GO_LIVE, &s(&[(DOMAINS, bad)])).is_err(), "{bad}");
    }
    for name in ["arugula", "studio", "hud", "planter-x", "flux-system", "shipped"] {
        assert!(!ship_name_ok(name), "{name} is reserved");
    }
    assert!(settings(SHIP, &s(&[])).unwrap_err().contains("needs app"));
    assert_eq!(
        settings(SHIP, &s(&[(APP, "hello")])).unwrap(),
        s(&[(APP, "hello"), (PORT, "8080"), (HEALTH, "/health"), (PORT_ENV, "PORT")])
    );
    assert!(settings(PROMOTE, &s(&[(APP, "hello")])).is_ok());
    let ok_prod = [
        s(&[(APP_DIR, "web/app"), (HEALTH, "/healthz"), (SECRETS, "true"), (POSTGRES, "true")]),
        s(&[(VOLUME, "2Gi"), (METRICS, "/metrics"), (PORT_ENV, "APP_PORT"), (DATA_ENV, "STATE")]),
        s(&[(VOLUME, "none")]),
    ];
    for ok in ok_prod {
        assert!(settings(PROD, &ok).is_ok(), "{ok:?}");
    }
    for (driver, bad) in [
        (PROD, s(&[(APP_DIR, "../x")])),
        (PROD, s(&[(APP_DIR, "/etc")])),
        (PROD, s(&[(APP_DIR, "-x")])),
        (PROD, s(&[(HEALTH, "health")])),
        (PROD, s(&[(SECRETS, "yes")])),
        (PROD, s(&[(VOLUME, "2GB")])),
        (PROD, s(&[(VOLUME, "0Gi")])),
        (PROD, s(&[(PORT_ENV, "1PORT")])),
        (PROD, s(&[(APP, "x")])),
        (SHIP, s(&[(APP, "Hello")])),
        (SHIP, s(&[(APP, &"a".repeat(33))])),
        (SHIP, s(&[(APP, "platform")])),
        (SHIP, s(&[(APP, "planter-x")])),
        (SHIP, s(&[(APP, "flux-system")])),
        (SHIP, s(&[(APP, "hello"), (PORT, "80")])),
        (SHIP, s(&[(APP, "hello"), (PORT, "08080")])),
        (SHIP, s(&[(APP, "hello"), (SECRETS, "A,,B")])),
        (SHIP, s(&[(APP, "hello"), (SECRETS, "A,A")])),
        (SHIP, s(&[(APP, "hello"), (VOLUME, "1Gi")])),
        (PROMOTE, s(&[])),
    ] {
        assert!(settings(driver, &bad).is_err(), "{driver} {bad:?}");
    }
}

/// What a page asks for is what [`settings`] takes, as for step 7's
/// drivers: each setting once, the required ones enough, the defaults the
/// ones filled in, each choice taken.
#[test]
fn setting_fields_are_the_settings_each_driver_takes() {
    for driver in DRIVERS {
        let f = fields_of(driver);
        let given: Settings = f.iter().filter(|f| f.required).map(|f| (f.name.to_owned(), "hello".into())).collect();
        let filled = settings_of(driver, &given).unwrap();
        for f in f {
            assert_eq!(filled.get(f.name).map(String::as_str), f.default.or(given.get(f.name).map(String::as_str)));
            for c in f.choices {
                let mut with = given.clone();
                with.insert(f.name.into(), (*c).into());
                assert!(settings_of(driver, &with).is_ok(), "{driver} {}={c}", f.name);
            }
        }
        let takes: Vec<&str> = f.iter().map(|f| f.name).collect();
        let mut unique = takes.clone();
        unique.dedup();
        assert_eq!(takes, unique, "{driver}");
    }
}

#[test]
fn each_driver_takes_its_own_source() {
    let sha = SHA;
    assert!(refuses(GO_LIVE, "commit", sha, "arugula-salad/ocarina").is_none());
    assert!(refuses(PROD, "commit", sha, "arugula-salad/ocarina").is_none());
    assert!(refuses(PROMOTE, "commit", sha, SHIPPED_REPO).is_none());
    assert!(refuses(SHIP, "artifact", &digest('a'), ARTIFACT_WHERE).is_none());
    for (driver, kind, r, w) in [
        (GO_LIVE, "commit", &sha[..12], "arugula-salad/ocarina"),
        (GO_LIVE, "artifact", sha, "arugula-salad/ocarina"),
        (GO_LIVE, "commit", sha, "acme/ocarina"),
        (GO_LIVE, "commit", sha, "arugula-salad/planter-apps"),
        (GO_LIVE, "commit", sha, &format!("arugula-salad/{}", "a".repeat(43))),
        (PROD, "commit", sha, "arugula-salad/Caps"),
        (PROMOTE, "commit", sha, "arugula-salad/ocarina"),
        (SHIP, "commit", sha, ARTIFACT_WHERE),
        (SHIP, "artifact", &digest('a').to_uppercase(), ARTIFACT_WHERE),
        (SHIP, "artifact", &digest('a')[..20], ARTIFACT_WHERE),
        (SHIP, "artifact", &digest('a'), SHIPPED_REPO),
    ] {
        assert!(refuses(driver, kind, r, w).is_some(), "{driver} {kind} {r} {w}");
        assert!(plan_lines(driver, kind, r, w, &Settings::new()).starts_with("refused "));
    }
    assert!(crate::drivers::source_ok(SHIP, "artifact", &digest('b')));
    assert!(!crate::drivers::source_ok(SHIP, "commit", SHA));
    assert!(crate::drivers::source_ok(GO_LIVE, "commit", SHA));
}

/// Each plan names the renderer by its digest, the repository it writes,
/// and the token it holds: planter's App, never the platform's.
#[test]
fn the_plans_say_what_they_write_and_with_what() {
    let asked = settings(GO_LIVE, &s(&[(DOMAINS, "shop.example.com")])).unwrap();
    let with = plan_text(GO_LIVE, "production", "commit", SHA, "arugula-salad/ocarina", &asked, None);
    assert!(
        with.contains("custom domains a new pointer asks for (platform.arugula.io/domains): shop.example.com;"),
        "{with}"
    );
    let live = plan_text(GO_LIVE, "production", "commit", SHA, "arugula-salad/ocarina", &Settings::new(), None);
    assert!(live.contains("(platform.arugula.io/domains): none;"), "{live}");
    for says in [
        format!("renderer planter's platform-app/render.mjs and prod-scaffold.mjs from {VENDORED_FROM}"),
        format!("sha256 {}", renderer_digest()),
        "repo arugula-salad/planter-apps (branch main)".into(),
        "path apps/ocarina.yaml".into(),
        format!("at commit {SHA}"),
        "platform.arugula.io/domains".into(),
        "https://ocarina.apps.arugula.io answers 200".into(),
        "planter's GitHub App (the deploy App; not the platform's)".into(),
    ] {
        assert!(live.contains(&says), "{says:?} in {live}");
    }
    let prod = settings(PROD, &s(&[(SECRETS, "true"), (VOLUME, "1Gi")])).unwrap();
    let p = plan_text(PROD, "production", "commit", SHA, "arugula-salad/ocarina", &prod, None);
    for says in [
        "setting app_dir app",
        "app/package.json",
        "branch planter/prod",
        "Deploy ocarina to prod",
        "volume 1Gi",
        "its secrets from Infisical",
        "writes arugula-salad/ocarina's contents, pull requests and workflows only",
    ] {
        assert!(p.contains(says), "{says:?} in {p}");
    }
    let ship = settings(SHIP, &s(&[(APP, "hello"), (SECRETS, "STRIPE_KEY")])).unwrap();
    let sp = plan_text(SHIP, "loam", "artifact", &digest('c'), ARTIFACT_WHERE, &ship, None);
    for says in [
        format!("take artifact {}", digest('c')),
        "arugula-salad/shipped's hello/".into(),
        "setting secrets STRIPE_KEY".into(),
        "https://hello.run.inevitable.fyi answers 200 at /health".into(),
        "apps/hello.yaml".into(),
        "no roll back".into(),
    ] {
        assert!(sp.contains(&says), "{says:?} in {sp}");
    }
    let pr = plan_text(PROMOTE, "cluster", "commit", SHA, SHIPPED_REPO, &s(&[(APP, "hello")]), None);
    for says in ["hello/app.yaml must say runtime loam", "https://hello.apps.inevitable.fyi", "branch promote/hello"] {
        assert!(pr.contains(says), "{says:?} in {pr}");
    }
}

/// The plan binds the driver, the settings and the source: a Ship plan
/// isn't a Promote's, another app is another plan, another artifact too.
#[test]
fn the_plan_binds_driver_settings_and_source() {
    let op = |driver: &str, kind: &str, r: &str, w: &str, st: &Settings| {
        let mut o = crate::deploy::Operation {
            project: "prj_1".into(),
            environment: "env_1".into(),
            environment_name: "loam".into(),
            plan: String::new(),
            source_kind: kind.into(),
            source_ref: r.into(),
            source_where: w.into(),
            planned: "h".into(),
        };
        o.plan = crate::drivers::plan_digest(driver, &o, st, None);
        o
    };
    let ship = settings(SHIP, &s(&[(APP, "hello")])).unwrap();
    let o = op(SHIP, "artifact", &digest('d'), ARTIFACT_WHERE, &ship);
    assert!(planned(SHIP, &o, &ship, None));
    let other = settings(SHIP, &s(&[(APP, "other")])).unwrap();
    assert!(!planned(SHIP, &o, &other, None), "another app");
    let port = settings(SHIP, &s(&[(APP, "hello"), (PORT, "3000")])).unwrap();
    assert!(!planned(SHIP, &o, &port, None), "another port");
    assert!(!planned(PROMOTE, &o, &ship, None), "another driver");
    let mut swapped = o.clone();
    swapped.source_ref = digest('e');
    assert!(!planned(SHIP, &swapped, &ship, None), "another artifact");
}
