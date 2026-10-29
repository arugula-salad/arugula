//! What a deploy is, in words (#333): the commit's subject, and for an
//! arugula-release environment the version the run is expected to tag.
//!
//! Read once, when the deploy is planned ([`read`]), best effort, and kept
//! beside the deploy ([`crate::db::deploys::Deploy::subject`]). **Never in
//! the plan, the operation or an event:** those are hashed and signed, and
//! a plan must stay reproducible from the commit and the settings
//! ([`super::plan_with`]), or every approval breaks. Pages show these as
//! the platform's reading, under the signed facts.

use std::time::Duration;

use arugula_e2e::drivers::arugula_release::{self as ar, BRANCH, BUMP, Lane, REPO};
use serde_json::Value;
use tracing::warn;

use crate::{
    Platform,
    db::deploys::{Environment, Source, SourceKind},
    forge::{Caller, Purpose, Repo},
};

/// A subject, at most this many characters.
pub const SUBJECT_MAX: usize = 100;

/// All the GitHub reads of one plan, together: a slow GitHub costs a plan
/// this long, never a failure.
const READ_FOR: Duration = Duration::from_secs(4);

/// What was read: each is `None` when it wasn't, or doesn't apply.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Facts {
    pub subject: Option<String>,
    pub expected_version: Option<String>,
}

/// Read the facts for a deploy of `source` to `env`, for its plan. Only a
/// commit has any. A failure or a timeout is a warning and leaves them
/// `None`: it never fails the plan.
pub async fn read(app: &Platform, env: &Environment, source: &Source) -> Facts {
    if source.kind != SourceKind::Commit {
        return Facts::default();
    }
    match tokio::time::timeout(READ_FOR, fetch(app, env, source)).await {
        Ok(Ok(facts)) => facts,
        Ok(Err(e)) => {
            warn!(env = %env.id, error = format!("{e:#}"), "deploy: couldn't read the commit for its title");
            Facts::default()
        }
        Err(_) => {
            warn!(env = %env.id, "deploy: reading the commit for its title timed out");
            Facts::default()
        }
    }
}

async fn fetch(app: &Platform, env: &Environment, source: &Source) -> anyhow::Result<Facts> {
    use anyhow::Context as _;
    let sha = source.reference.as_str();
    // Goes into a URL: a full or abbreviated commit id, nothing else.
    if !(7..=64).contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(Facts::default());
    }
    let gh = app.github.as_ref().context("this platform has no GitHub App")?;
    let release = env.driver == ar::DRIVER;
    // Arugula's own repository is the release driver's alone. Any other is
    // the planner's to name, so only one the project links is read.
    let (repo, install, id) = if release {
        let i = gh.installation(&app.http, REPO).await?.context("the App isn't installed on the repository")?;
        (REPO.to_owned(), i.id, None)
    } else {
        let l = super::mint::source_link(app, &env.project, source)?.context("the repository isn't linked")?;
        (l.full_name.clone(), l.installation_id, Some(l.repo_id))
    };
    let name = repo.as_str();
    let target = id.map_or(Repo::Name(name), Repo::Id);
    let token = gh.read_token(&Caller::of(app, "platform"), install, target, Purpose::Read).await?;
    let api = gh.api.trim_end_matches('/');
    let get = async |path: String| -> anyhow::Result<Value> {
        let r = app
            .http
            .get(format!("{api}/repos/{name}/{path}"))
            .bearer_auth(token.secret())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "arugula-platform")
            .send()
            .await?
            .error_for_status()?;
        Ok(r.json().await?)
    };
    let mut facts = Facts::default();
    match get(format!("commits/{sha}")).await {
        Ok(c) => facts.subject = c["commit"]["message"].as_str().and_then(subject_of),
        Err(e) => warn!(env = %env.id, error = format!("{e:#}"), "deploy: couldn't read the commit's subject"),
    }
    if release {
        let lane = Lane::of(&env.settings);
        let file = match lane {
            Lane::Daemon => "Cargo.toml",
            Lane::App => "crates/desktop/Cargo.toml",
        };
        let read_at = async |at: &str| -> anyhow::Result<String> {
            let v = get(format!("contents/{file}?ref={at}")).await?;
            file_text(&v).context("no file")
        };
        match (read_at(sha).await, read_at(BRANCH).await) {
            (Ok(at_sha), Ok(at_main)) => {
                let bump = env.settings.get(BUMP).map_or("patch", String::as_str);
                facts.expected_version = expected_version(lane, &at_sha, &at_main, bump);
            }
            (a, b) => {
                let e = a.err().or(b.err()).map(|e| format!("{e:#}"));
                warn!(env = %env.id, error = ?e, "deploy: couldn't read the versions to work out the release's");
            }
        }
    }
    Ok(facts)
}

/// A file as GitHub's contents API answers it (base64 with newlines).
fn file_text(v: &Value) -> Option<String> {
    use base64::Engine as _;
    let clean: String = v["content"].as_str()?.chars().filter(|c| !c.is_whitespace()).collect();
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(clean).ok()?).ok()
}

/// A commit message's first line, trimmed, at most [`SUBJECT_MAX`]
/// characters (an ellipsis after a cut). `None` when it's empty.
pub fn subject_of(message: &str) -> Option<String> {
    let line = message.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= SUBJECT_MAX {
        return Some(line.to_owned());
    }
    let cut: String = line.chars().take(SUBJECT_MAX - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// The version a release of `lane` is expected to tag: the way `bump` in
/// `scripts/release` works it out. From the higher of the commit's version
/// and main's, one `bump` (`patch`, `minor` or `major`) up. `None` when a
/// version isn't there or isn't `x.y.z`.
pub fn expected_version(lane: Lane, at_commit: &str, at_main: &str, bump: &str) -> Option<String> {
    let section = match lane {
        Lane::Daemon => "[workspace.package]",
        Lane::App => "[package]",
    };
    let a = triple(&version_in(at_commit, section)?)?;
    let b = triple(&version_in(at_main, section)?)?;
    let (major, minor, patch) = a.max(b);
    Some(match bump {
        "major" => format!("{}.0.0", major + 1),
        "minor" => format!("{major}.{}.0", minor + 1),
        "patch" => format!("{major}.{minor}.{}", patch + 1),
        _ => return None,
    })
}

/// The first `version = "…"` in a manifest's `section` (`version_at` in
/// `scripts/release`).
fn version_in(toml: &str, section: &str) -> Option<String> {
    let mut inside = false;
    for line in toml.lines() {
        if line.starts_with('[') {
            inside = line == section;
        } else if inside && let Some(v) = line.strip_prefix("version = \"") {
            return v.strip_suffix('"').map(str::to_owned);
        }
    }
    None
}

fn triple(v: &str) -> Option<(u64, u64, u64)> {
    let mut p = v.split('.').map(|n| n.parse::<u64>().ok());
    let t = (p.next()??, p.next()??, p.next()??);
    p.next().is_none().then_some(t)
}

/// A deploy's title: what it deploys, then where or what it is. One
/// wording with the pages' (`deployTitle` in web/src/shared/deploys.ts). A
/// release's version is expected, never certain. No facts: just where.
pub fn title(env: &Environment, source: &Source, subject: Option<&str>, expected: Option<&str>) -> String {
    let short: String = source.reference.chars().take(8).collect();
    let with = |head: String| match subject {
        Some(s) => format!("{head} · {s}"),
        None => head,
    };
    match (expected, subject) {
        (Some(v), _) if env.driver == ar::DRIVER => with(format!("Release {v} (expected) · {short}")),
        (_, Some(_)) => with(format!("Deploy {short} to {}", env.name)),
        _ => format!("Deploy to {}", env.name),
    }
}
