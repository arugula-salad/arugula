//! planter's flows as deploy drivers (M233, platform step 8, native
//! Arugula #31; aru-104, aru-108): what the platform and a deploy's box both
//! need to agree on, so the plan an owner approves on their device names
//! what will change, and the box recomputes it before it applies.
//!
//! planter (arugula-salad/planter) put apps on home-cloud's app platform
//! four ways, each now a driver an environment names:
//!
//! - **Prod** ([`PROD`]): the pull request that gives an app's repository
//!   what prod needs (a Dockerfile, the build-and-pin workflow, `app.yaml`
//!   and its `k8s/`), from planter's own scaffold. A person merges it.
//! - **Go live** ([`GO_LIVE`]): the app's pointer in [`POINTERS_REPO`],
//!   which Flux applies at `https://<name>.apps.arugula.io`. It's
//!   apps-platform's apply (M222), with planter's pointer: the custom
//!   domains the app's `app.yaml` asks for go on it.
//! - **Ship** ([`SHIP`]): what an agent built, a gzipped tar with a
//!   `Dockerfile` at its root, into [`SHIPPED_REPO`] as `<app>/src/` with
//!   `app.yaml` for loam and its render, at `https://<app>.run.inevitable.fyi`.
//! - **Promote** ([`PROMOTE`]): the pull request that moves a shipped app
//!   from loam into the cluster. A person merges it (planter's decision 5).
//!
//! **planter's renderer, vendored** ([`RENDERER`]): `render.mjs` and
//! `prod-scaffold.mjs`, byte for byte, with a shim of ours
//! (`planter/renderer.mjs`). The box writes them into its run's directory and
//! asks `node`, so an app gets exactly what planter gave it. The plan names
//! their digest, so the approval binds the renderer too.
//!
//! **planter's GitHub App stays the deploy App** (aru-108): every token
//! these drivers' runs get is an installation token of planter's App, minted
//! for that run, for one repository; the platform's own App isn't asked for
//! anything more.

use sha2::{Digest as _, Sha256};

use super::apps_platform;
use crate::drivers::{SettingField, Settings};

/// Its drivers' names, as environments name them.
pub const PROD: &str = "planter-prod";
pub const GO_LIVE: &str = "planter-go-live";
pub const SHIP: &str = "planter-ship";
pub const PROMOTE: &str = "planter-promote";
pub const DRIVERS: [&str; 4] = [PROD, GO_LIVE, SHIP, PROMOTE];

/// Whether `driver` is one of planter's.
pub fn is_planter(driver: &str) -> bool {
    DRIVERS.contains(&driver)
}

/// The organization planter's apps are repositories of.
pub const ORG: &str = "arugula-salad";
/// Where the pointers are: home-cloud's list for apps on GitHub, as M222's.
pub const POINTERS_REPO: &str = apps_platform::LIST_REPO;
/// The list's branch Flux applies.
pub const POINTERS_BRANCH: &str = apps_platform::LIST_BRANCH;
/// Where shipped apps live, one directory each.
pub const SHIPPED_REPO: &str = "arugula-salad/shipped";
pub const SHIPPED_BRANCH: &str = "main";
/// An app in the cluster: `https://<name>.apps.arugula.io`.
pub const APPS_DOMAIN: &str = apps_platform::APPS_DOMAIN;
/// An app on loam: `https://<name>.run.inevitable.fyi`.
pub const LOAM_DOMAIN: &str = "run.inevitable.fyi";
/// A promoted app: `https://<name>.apps.inevitable.fyi`.
pub const SECOND_DOMAIN: &str = "apps.inevitable.fyi";
/// The branch Prod's pull request comes from.
pub const SCAFFOLD_BRANCH: &str = "planter/prod";

/// The branch a promotion's pull request comes from.
pub fn promote_branch(app: &str) -> String {
    format!("promote/{app}")
}

/// The names the runs get their tokens under: planter's App's installation
/// tokens, minted for the run.
/// - [`POINTERS_TOKEN`]: writes [`POINTERS_REPO`]'s contents (Go live, Ship);
/// - [`SOURCE_TOKEN`]: reads the app's repository (Go live);
/// - [`APP_TOKEN`]: writes the app's repository's contents, pull requests
///   and workflows (Prod: its pull request adds `.github/workflows/build.yml`);
/// - [`SHIPPED_TOKEN`]: writes [`SHIPPED_REPO`]'s contents (Ship), and its
///   pull requests (Promote).
pub const POINTERS_TOKEN: &str = "PLANTER_APPS_TOKEN";
pub const SOURCE_TOKEN: &str = "SOURCE_TOKEN";
pub const APP_TOKEN: &str = "APP_REPO_TOKEN";
pub const SHIPPED_TOKEN: &str = "SHIPPED_TOKEN";

/// The secret names each of planter's drivers takes: only tokens it mints.
pub fn secrets(driver: &str) -> Option<&'static [&'static str]> {
    Some(match driver {
        PROD => &[APP_TOKEN],
        GO_LIVE => &[POINTERS_TOKEN, SOURCE_TOKEN],
        SHIP => &[SHIPPED_TOKEN, POINTERS_TOKEN],
        PROMOTE => &[SHIPPED_TOKEN],
        _ => return None,
    })
}

// ---- planter's renderer, vendored ----

/// Where it came from: planter's commit, as `scripts/vendor-planter` copied it.
pub const VENDORED_FROM: &str = "arugula-salad/planter@90d6e772213c78301b92e89c01377197973b54fc";

/// The files a box writes into its run's directory, by the path each has
/// there, and runs `node renderer.mjs` in it: planter's two (unchanged but
/// for the lines naming where they came from) and our shim.
pub const RENDERER: [(&str, &str); 3] = [
    ("platform-app/render.mjs", include_str!("planter/platform-app/render.mjs")),
    ("prod-scaffold.mjs", include_str!("planter/prod-scaffold.mjs")),
    ("renderer.mjs", include_str!("planter/renderer.mjs")),
];

/// The renderer's digest: SHA-256 over each file's path and bytes, hex.
pub fn renderer_digest() -> String {
    let mut h = Sha256::new();
    for (path, text) in RENDERER {
        h.update(path.as_bytes());
        h.update([0]);
        h.update(text.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())
}

// ---- names ----

/// An app's name in prod (planter's `prodName`): a host label, a Kubernetes
/// and a ghcr name, at most 42.
pub fn prod_name_ok(name: &str) -> bool {
    label(name, 42)
}

/// A shipped app's name (planter's ship): a loam sandbox's, at most 32,
/// and not one the platform keeps.
pub fn ship_name_ok(name: &str) -> bool {
    label(name, 32) && !reserved(name)
}

/// Names nobody ships under (M233's review: a reserved list, besides
/// asking GitHub): the org's own products and services, and names a host
/// should never be.
pub const RESERVED_NAMES: &[&str] = &[
    "arugula",
    "studio",
    "hud",
    "fleet",
    "redone",
    "experimental",
    "home-cloud",
    "control",
    "hub",
    "api",
    "www",
    "admin",
    "auth",
    "login",
    "signin",
    "oauth",
    "mail",
    "smtp",
    "status",
    "docs",
    "loam",
    "fountain",
    "managoat",
    "chant",
    "beds",
    "lobby",
    "door",
    "grafana",
    "infisical",
    "flux",
    "github",
    "registry",
    "ghcr",
];

/// Names planter never let an agent ship under: the platform's own
/// (`platform`, `platform-*`), `shipped`, planter's (`planter*`), `app`
/// and `apps`; what Flux keeps beside the pointers (M222's); and
/// [`RESERVED_NAMES`].
pub fn reserved(name: &str) -> bool {
    name == "platform"
        || name.starts_with("platform-")
        || name == "shipped"
        || name.starts_with("planter")
        || name == "app"
        || name == "apps"
        || apps_platform::RESERVED_APP_NAMES.contains(&name)
        || RESERVED_NAMES.contains(&name)
}

fn label(name: &str, max: usize) -> bool {
    (1..=max).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// The app a commit's `where` names, for Prod and Go live: `arugula-salad/<name>`,
/// a name prod takes, not one the list keeps.
pub fn app_of(source_where: &str) -> Option<&str> {
    let (owner, name) = source_where.split_once('/')?;
    (owner == ORG && prod_name_ok(name) && !reserved(name)).then_some(name)
}

fn full_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A shipped source's reference: `sha256:` and 64 lowercase hex.
pub fn artifact_ok(reference: &str) -> bool {
    reference
        .strip_prefix("sha256:")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// A shipped source's `where`: the platform's own store, where an agent's
/// ship put it.
pub const ARTIFACT_WHERE: &str = "platform";

/// The variable naming the projects planter's drivers may be used in,
/// comma separated (M233's review): none when it's unset or empty.
pub const PROJECTS_VAR: &str = "PLANTER_PROJECTS";

/// The most a project's kept artifacts may add up to, in bytes.
pub const MAX_PROJECT_ARTIFACTS: u64 = 200 * 1024 * 1024;

/// The most custom domains Go live asks for.
pub const MAX_DOMAINS: usize = 10;

/// The most a ship's tarball may be, as sent (planter took 16 MiB; the
/// platform keeps it in its database until its deploy has run, and it
/// comes through MCP, so less).
pub const MAX_TARBALL: usize = 2 * 1024 * 1024;
/// What one ship may hold once unpacked (planter's limits).
pub const MAX_FILES: usize = 2000;
pub const MAX_UNPACKED: u64 = 64 * 1024 * 1024;

// ---- settings ----

/// Prod's settings: where the app member is, and what the app's own files
/// don't say (planter's Prod read them through chant; here they're the
/// environment's, so the plan names them).
pub const APP_DIR: &str = "app_dir";
pub const HEALTH: &str = "health";
pub const PORT_ENV: &str = "port_env";
pub const DATA_ENV: &str = "data_env";
pub const SECRETS: &str = "secrets";
pub const POSTGRES: &str = "postgres";
pub const METRICS: &str = "metrics";
pub const VOLUME: &str = "volume";
/// Ship's and Promote's: the app, and (Ship) what its `app.yaml` says.
pub const APP: &str = "app";
pub const PORT: &str = "port";
/// Go live's: the custom domains a new pointer asks for, space separated,
/// which the plan lists and the app's `app.yaml` must ask for exactly.
pub const DOMAINS: &str = "domains";

const PROD_SETTINGS: &[&str] = &[APP_DIR, HEALTH, PORT_ENV, DATA_ENV, SECRETS, POSTGRES, METRICS, VOLUME];
const SHIP_SETTINGS: &[&str] = &[APP, PORT, HEALTH, PORT_ENV, SECRETS];
const PROMOTE_SETTINGS: &[&str] = &[APP];
const GO_LIVE_SETTINGS: &[&str] = &[DOMAINS];

/// A host name a custom domain may be: lowercase labels, a dot, at most 253.
pub fn host_ok(h: &str) -> bool {
    h.len() <= 253
        && h.contains('.')
        && h.split('.').all(|l| {
            (1..=63).contains(&l.len())
                && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
}

/// Go live's custom domains, as the setting lists them (space separated).
pub fn domains(settings: &Settings) -> Vec<String> {
    settings
        .get(DOMAINS)
        .map(|d| d.split(' ').filter(|h| !h.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn env_name_ok(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A path the app answers (planter's `PATH`): `/` then letters, digits and
/// `._~/-`.
pub fn path_ok(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 200
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'/' | b'-'))
}

fn size_ok(s: &str) -> bool {
    let n = s.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = &s[n.len()..];
    ["Mi", "Gi", "Ti"].contains(&unit)
        && !n.is_empty()
        && n.len() <= 6
        && !n.starts_with('0')
        && n.bytes().all(|b| b.is_ascii_digit())
}

fn dir_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && !s.starts_with('/')
        && !s.contains("..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

/// `driver`'s settings as an environment keeps them, defaults filled in:
/// `Err` why not.
pub fn settings(driver: &str, given: &Settings) -> Result<Settings, String> {
    let takes = match driver {
        PROD => PROD_SETTINGS,
        SHIP => SHIP_SETTINGS,
        PROMOTE => PROMOTE_SETTINGS,
        GO_LIVE => GO_LIVE_SETTINGS,
        _ => &[],
    };
    if let Some(k) = given.keys().find(|k| !takes.contains(&k.as_str())) {
        return Err(if takes.is_empty() {
            format!("the {driver} driver takes no settings ({k})")
        } else {
            format!("{driver} has no setting {k}: it takes {}", takes.join(", "))
        });
    }
    // No value reads as a flag.
    if let Some((k, _)) = given.iter().find(|(_, v)| v.is_empty() || v.starts_with('-') || v.len() > 200) {
        return Err(format!("{k} is empty, too long, or starts with -"));
    }
    let mut s = given.clone();
    match driver {
        PROD => {
            s.entry(APP_DIR.into()).or_insert_with(|| "app".into());
            if !dir_ok(&s[APP_DIR]) {
                return Err("app_dir is the app member's directory, from the repository's root".into());
            }
        }
        SHIP => {
            match s.get(APP) {
                Some(a) if ship_name_ok(a) => {}
                Some(a) => {
                    return Err(format!(
                        "app {a}: lowercase letters, digits and dashes, at most 32 (its sandbox's name), and not \
                         one the platform keeps"
                    ));
                }
                None => return Err("planter-ship needs app: the shipped app's name, its host's".into()),
            }
            s.entry(PORT.into()).or_insert_with(|| "8080".into());
            s.entry(HEALTH.into()).or_insert_with(|| "/health".into());
            s.entry(PORT_ENV.into()).or_insert_with(|| "PORT".into());
            if !s[PORT].parse::<u16>().is_ok_and(|p| p >= 1024) || s[PORT].starts_with('0') {
                return Err("port is the port the app listens on, 1024 to 65535".into());
            }
        }
        PROMOTE => match s.get(APP) {
            Some(a) if ship_name_ok(a) => {}
            _ => return Err("planter-promote needs app: the shipped app it promotes".into()),
        },
        GO_LIVE => {
            if let Some(d) = s.get(DOMAINS) {
                let hosts: Vec<&str> = d.split(' ').collect();
                if hosts.len() > MAX_DOMAINS
                    || !hosts.iter().all(|h| host_ok(h))
                    || hosts.iter().enumerate().any(|(i, h)| hosts[..i].contains(h))
                {
                    return Err(format!("domains is up to {MAX_DOMAINS} host names, space separated, each once"));
                }
            }
        }
        _ => {}
    }
    if s.get(HEALTH).is_some_and(|h| !path_ok(h)) || s.get(METRICS).is_some_and(|m| !path_ok(m)) {
        return Err("health and metrics are paths the app answers, such as /health".into());
    }
    for k in [PORT_ENV, DATA_ENV] {
        if s.get(k).is_some_and(|v| !env_name_ok(v)) {
            return Err(format!("{k} is an environment variable's name"));
        }
    }
    if driver == PROD {
        for flag in [SECRETS, POSTGRES] {
            if s.get(flag).is_some_and(|v| v != "true") {
                return Err(format!("{flag} is true, or left out"));
            }
        }
        if s.get(VOLUME).is_some_and(|v| v != "none" && !size_ok(v)) {
            return Err("volume is a size such as 1Gi, or none".into());
        }
    }
    if driver == SHIP
        && let Some(list) = s.get(SECRETS)
    {
        let names: Vec<&str> = list.split(',').collect();
        if !names.iter().all(|n| env_name_ok(n)) {
            return Err("secrets is environment variables' names, comma separated".into());
        }
        if names.iter().enumerate().any(|(i, n)| names[..i].contains(n)) {
            return Err("secrets names a secret more than once".into());
        }
    }
    Ok(s)
}

const fn field(
    name: &'static str,
    required: bool,
    default: Option<&'static str>,
    choices: &'static [&'static str],
    about: &'static str,
) -> SettingField {
    SettingField { name, required, default, choices, about }
}

const PROD_FIELDS: &[SettingField] = &[
    field(APP_DIR, false, Some("app"), &[], "The app member's directory: the image's build context."),
    field(HEALTH, false, None, &[], "Its health path, when it isn't /health."),
    field(PORT_ENV, false, None, &[], "The variable it reads its port from, when it isn't PORT."),
    field(DATA_ENV, false, None, &[], "The variable it reads its data directory from, when it isn't APP_DATA."),
    field(SECRETS, false, None, &["true"], "Load its secrets from Infisical as env."),
    field(POSTGRES, false, None, &["true"], "A database of its own on apps-pg, as DATABASE_URL."),
    field(METRICS, false, None, &[], "Its Prometheus metrics path, scraped."),
    field(VOLUME, false, None, &[], "A volume at /data of this size (1Gi), or none; one if it migrates, else none."),
];

const SHIP_FIELDS: &[SettingField] = &[
    field(APP, true, None, &[], "The app's name: https://<app>.run.inevitable.fyi."),
    field(PORT, false, Some("8080"), &[], "The port it listens on."),
    field(HEALTH, false, Some("/health"), &[], "The path that answers 200 once it's up."),
    field(PORT_ENV, false, Some("PORT"), &[], "The variable it reads its port from."),
    field(SECRETS, false, None, &[], "The secrets it's given as env, by name, comma separated."),
];

const GO_LIVE_FIELDS: &[SettingField] = &[field(
    DOMAINS,
    false,
    None,
    &[],
    "Custom domains a new pointer asks for, space separated: exactly those its app.yaml routes beyond its own two.",
)];

const PROMOTE_FIELDS: &[SettingField] =
    &[field(APP, true, None, &[], "The shipped app it moves from loam into the cluster.")];

/// The settings `driver` takes, in the order a page asks for them.
pub fn setting_fields(driver: &str) -> &'static [SettingField] {
    match driver {
        PROD => PROD_FIELDS,
        SHIP => SHIP_FIELDS,
        PROMOTE => PROMOTE_FIELDS,
        GO_LIVE => GO_LIVE_FIELDS,
        _ => &[],
    }
}

// ---- sources and plans ----

/// Whether `driver` deploys a source of this kind and reference: Ship an
/// artifact by its digest, the others a commit by its full id.
pub fn source_ok(driver: &str, kind: &str, reference: &str) -> bool {
    match driver {
        SHIP => kind == "artifact" && artifact_ok(reference),
        PROD | GO_LIVE | PROMOTE => kind == "commit" && full_commit(reference),
        _ => true,
    }
}

/// Why `driver` can't deploy a source, before anything is planned: `None`
/// when it can.
pub fn refuses(driver: &str, kind: &str, reference: &str, source_where: &str) -> Option<String> {
    if !source_ok(driver, kind, reference) {
        return Some(if driver == SHIP {
            format!("the {SHIP} driver deploys what an agent shipped: an artifact, sha256:<64 hex>")
        } else {
            format!("the {driver} driver deploys a commit, by its full id (40 lowercase hex)")
        });
    }
    match driver {
        PROD | GO_LIVE if app_of(source_where).is_none() => Some(format!(
            "the {driver} driver deploys {ORG}/<app> repositories, named as an app in prod is (lowercase letters, \
             digits and dashes, at most 42)"
        )),
        PROMOTE if source_where != SHIPPED_REPO => {
            Some(format!("the {PROMOTE} driver promotes a commit of {SHIPPED_REPO}"))
        }
        SHIP if source_where != ARTIFACT_WHERE => {
            Some(format!("the {SHIP} driver ships an artifact the platform keeps (where: {ARTIFACT_WHERE})"))
        }
        _ => None,
    }
}

fn get<'a>(s: &'a Settings, k: &str) -> Option<&'a str> {
    s.get(k).map(String::as_str)
}

/// What `driver` does, in words: the part of its plan a person reads before
/// they approve (after the settings lines `plan_text` writes).
pub fn plan_lines(driver: &str, kind: &str, reference: &str, source_where: &str, settings: &Settings) -> String {
    if let Some(why) = refuses(driver, kind, reference, source_where) {
        return format!("refused {why}\n");
    }
    let mut t = format!(
        "renderer planter's platform-app/render.mjs and prod-scaffold.mjs from {VENDORED_FROM}, run with node on \
         the box (sha256 {})\n",
        renderer_digest()
    );
    let mint = "an installation token of planter's GitHub App (the deploy App; not the platform's), minted for this \
                run";
    match driver {
        GO_LIVE => {
            let name = app_of(source_where).unwrap_or_default();
            let path = apps_platform::pointer_path(name);
            let asked = domains(settings);
            let asked = if asked.is_empty() { "none".to_owned() } else { asked.join(", ") };
            t.push_str(&format!(
                "repo {POINTERS_REPO} (branch {POINTERS_BRANCH})\npath {path}\nchange pin the GitRepository {name} \
                 (https://github.com/{source_where}) at commit {reference}; with no pointer, a new one as planter's Go \
                 live writes it; a revert commit when it rolls back the last deploy\n\
                 custom domains a new pointer asks for (platform.arugula.io/domains): {asked}; that commit's \
                 app.yaml must route exactly these beyond its own two hosts (sorted by planter's validate()), or \
                 nothing is pushed. An existing pointer's are left as they are\n\
                 check the commit is on {source_where}'s default branch, its app.yaml names {name} and runtime k8s, \
                 and no other app holds the name\n\
                 verify https://{name}.{APPS_DOMAIN} answers 200 at its app.yaml's health path; when the answer \
                 reports a revision (JSON `revision`, `commit`, `version` or `sha`, or an `X-Revision` header), it \
                 must be this commit's or its pinned image's\n\
                 with: {mint}, that writes {POINTERS_REPO}'s contents only, and one that reads {source_where}'s \
                 contents only; both revoked right after the push\n"
            ));
        }
        PROD => {
            let name = app_of(source_where).unwrap_or_default();
            let dir = get(settings, APP_DIR).unwrap_or("app");
            let mut chose = vec![];
            for (k, says) in [
                (HEALTH, "health"),
                (PORT_ENV, "port from"),
                (DATA_ENV, "data directory from"),
                (METRICS, "metrics at"),
                (VOLUME, "volume"),
            ] {
                if let Some(v) = get(settings, k) {
                    chose.push(format!("{says} {v}"));
                }
            }
            for (k, says) in [(SECRETS, "its secrets from Infisical"), (POSTGRES, "a database on apps-pg")] {
                if get(settings, k).is_some() {
                    chose.push(says.to_owned());
                }
            }
            let chose = if chose.is_empty() { String::new() } else { format!(" ({})", chose.join(", ")) };
            t.push_str(&format!(
                "does: read {source_where} at commit {reference}, on its default branch: {dir}/package.json (its \
                 start, test and migrate scripts and node's version) and whether {dir}/package-lock.json is there; \
                 stop if the repository has k8s/kustomization.yaml already\n\
                 does: write planter's Prod scaffold for https://{name}.{APPS_DOMAIN}{chose}: Dockerfile, \
                 Dockerfile.dockerignore, .github/workflows/build.yml, app.yaml, k8s/kustomization.yaml and \
                 k8s/platform/\n\
                 does: commit them on branch {SCAFFOLD_BRANCH} from that commit, push it, and open the pull request \
                 \u{201c}Deploy {name} to prod\u{201d} into the default branch (or find the one open); a person \
                 merges it, its first build pins the image, and a {GO_LIVE} deploy puts it live\n\
                 with: {mint}, that writes {source_where}'s contents, pull requests and workflows only, revoked \
                 right after\n"
            ));
        }
        SHIP => {
            let app = get(settings, APP).unwrap_or_default();
            let health = get(settings, HEALTH).unwrap_or("/health");
            t.push_str(&format!(
                "does: take artifact {reference} from the platform's store (what an agent shipped), check its digest \
                 and unpack it: a gzipped tar, regular files only, at most {MAX_FILES} files and {} MiB, with a \
                 Dockerfile at its root\n\
                 does: write {SHIPPED_REPO}'s {app}/ in one commit on {SHIPPED_BRANCH}: src/ (the files), app.yaml \
                 for runtime loam as planter's ship writes it (the port, health path, port variable and secrets in \
                 the settings), k8s/platform/ rendered from it, and k8s/kustomization.yaml if there's none; stop if \
                 {app}/app.yaml says another runtime (a promoted app changes by pull request only)\n\
                 does: the first time, point {POINTERS_REPO}'s apps/{app}.yaml at {SHIPPED_REPO}'s {app}/k8s (branch \
                 {SHIPPED_BRANCH}), as planter did; stop if another app holds the name, it's a repository of {ORG}'s \
                 (asked of GitHub), or (with secrets) anyone else ever shipped or pointed at {app}\n\
                 verify {SHIPPED_REPO}'s build pins {app}'s image by digest after this commit, then \
                 https://{app}.{LOAM_DOMAIN} answers 200 at {health}, within 20 minutes\n\
                 with: {mint}, that writes {SHIPPED_REPO}'s contents only, and one that writes {POINTERS_REPO}'s \
                 contents only; both revoked right after the push\n\
                 no roll back: ship the source to go back to again\n",
                MAX_UNPACKED >> 20,
            ));
        }
        PROMOTE => {
            let app = get(settings, APP).unwrap_or_default();
            t.push_str(&format!(
                "does: read {SHIPPED_REPO} at commit {reference}, on its {SHIPPED_BRANCH}: {app}/app.yaml must say \
                 runtime loam\n\
                 does: write {app}/app.yaml for runtime k8s at https://{app}.{SECOND_DOMAIN} and {app}/k8s/platform/ \
                 rendered from it, as planter's promote does, on branch {} from that commit, and open the pull \
                 request \u{201c}Promote {app} from loam to the cluster\u{201d} (or find the one open); a person \
                 merging it is the review, and moves the app\n\
                 with: {mint}, that writes {SHIPPED_REPO}'s contents and pull requests only, revoked right after\n\
                 no roll back: close the pull request\n",
                promote_branch(app),
            ));
        }
        _ => {}
    }
    t
}

#[cfg(test)]
#[path = "deploy_planter_tests.rs"]
mod tests;
