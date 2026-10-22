//! The arugula-release driver (M221, platform step 7, native Arugula #17):
//! Arugula's own release as a deploy, approved by an owner's device,
//! instead of `just release` from Jake's shell.
//!
//! - **Plan** ([`arugula_e2e::drivers::arugula_release::plan_lines`]): the
//!   commit, the bump and the lane (its settings), what `scripts/release
//!   bump` does with them, the token, and that it never rolls back. Only a
//!   commit of [`REPO`] is planned ([`Driver::refuses`]).
//! - **Only where the platform says so** ([`Driver::allowed_in`]): the one
//!   project `ARUGULA_PLATFORM_RELEASE_PROJECT` names (Arugula's own). No
//!   other project may make an environment for it, plan with it, or have
//!   its token minted, so no one else's approval can push to Arugula.
//! - **Apply** runs on the deploy's box (`arugulad _arugula-release`): it
//!   clones [`REPO`], checks the commit is on `main` and green, runs the
//!   commit's own `scripts/release bump`, which tags and pushes `main` and
//!   the tag atomically, revokes the token, and follows the tag's release
//!   workflow to its end.
//! - **Its tokens** are minted for the run, once the platform has checked
//!   with GitHub that the commit is on `main` and green ([`source_checked`]):
//!   an installation token of the platform's GitHub App, `contents: write`
//!   on [`REPO`] alone ([`Mint::GithubWrite`], [`TOKEN`]), and one that
//!   only reads its Actions runs and contents ([`Mint::GithubFollow`],
//!   [`READ_TOKEN`]), to follow the workflow within GitHub's limits.
//!   Nothing else: the release workflow's own secrets stay in GitHub
//!   Actions.
//! - **Verify:** the run exited 0, and it reported the tag it pushed (of
//!   the environment's lane) and that the release workflow published it.
//!   The log keeps the tag, the commits, the release's page and the
//!   workflow's run.
//! - **No roll back:** a bad release is fixed by a new patch release.

use arugula_e2e::drivers::{
    arugula_release::{self as ar, BRANCH, Lane, READ_TOKEN, REPO, TOKEN},
    word,
};
use serde_json::{Map, Value};

use super::{Driver, Mint, Verdict, exited_0};
use crate::{
    Platform,
    db::deploys::{Deploy, Environment, Source, SourceKind},
    db::runs::Run,
    forge::{Caller, Purpose, Repo},
};

/// Its name, as environments name it.
pub const NAME: &str = ar::DRIVER;

/// The platform's environment variable that names the project (or
/// projects, comma separated) whose environments may release Arugula.
pub const PROJECT_VAR: &str = "ARUGULA_PLATFORM_RELEASE_PROJECT";

pub struct ArugulaRelease;

/// Tests: projects allowed besides what [`PROJECT_VAR`] names (each test's
/// own, so tests don't share an environment variable).
#[cfg(test)]
pub static TEST_PROJECTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether `project` is one `named` (the variable's value) lists: none
/// when it's unset or empty.
pub fn project_named(named: Option<&str>, project: &str) -> bool {
    named.is_some_and(|n| n.split(',').map(str::trim).any(|p| !p.is_empty() && p == project))
}

/// A string in `v` at `k`, if it's one word.
fn at<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str().filter(|s| word(s))
}

/// Whether `s` is a full commit id.
fn commit_ok(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The tag the box reported it pushed, if it's one of the environment's
/// lane.
fn tag<'a>(env: &Environment, reported: &'a Value) -> Option<&'a str> {
    at(reported, "tag").filter(|t| ar::tag_version(Lane::of(&env.settings), t).is_some())
}

impl Driver for ArugulaRelease {
    fn name(&self) -> &'static str {
        NAME
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        ar::refuses(source.kind.as_str(), &source.reference, &source.place)
    }

    fn allowed_in(&self, project: &str) -> Result<(), String> {
        #[cfg(test)]
        if TEST_PROJECTS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == project) {
            return Ok(());
        }
        if project_named(std::env::var(PROJECT_VAR).ok().as_deref(), project) {
            Ok(())
        } else {
            Err(format!("the {NAME} driver releases Arugula itself: only Arugula's own project may use it"))
        }
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![Mint::GithubWrite { name: TOKEN, repo: REPO }, Mint::GithubFollow { name: READ_TOKEN, repo: REPO }]
    }

    fn verify(&self, env: &Environment, _: &Deploy, run: &Run, reported: &Value) -> Verdict {
        if let Verdict::Unhealthy(why) = exited_0(run) {
            return Verdict::Unhealthy(why);
        }
        let Some(t) = tag(env, reported) else {
            return Verdict::Unhealthy("the apply didn't say which tag it pushed".into());
        };
        if !at(reported, "commit").is_some_and(commit_ok) {
            return Verdict::Unhealthy("the apply didn't say which commit it tagged".into());
        }
        if reported["published"] != Value::Bool(true) || at(reported, "release") != Some(ar::release_url(t).as_str()) {
            return Verdict::Unhealthy(format!("the release workflow didn't publish {t}"));
        }
        Verdict::Healthy
    }

    fn kept(&self, env: &Environment, _: &Deploy, reported: &Value) -> Map<String, Value> {
        let mut m = Map::new();
        let Some(t) = tag(env, reported) else { return m };
        m.insert("tag".into(), t.into());
        m.insert("release".into(), ar::release_url(t).into());
        for k in ["commit", "main", "script"] {
            if let Some(c) = at(reported, k).filter(|c| commit_ok(c)) {
                m.insert(k.into(), c.into());
            }
        }
        let wf = &reported["workflow"];
        if let Some(u) = at(wf, "run").filter(|u| ar::run_url_ok(u)) {
            m.insert("workflow_run".into(), u.into());
        }
        if let Some(c) = at(wf, "conclusion").filter(|c| c.len() <= 40) {
            m.insert("workflow_conclusion".into(), c.into());
        }
        m.insert("published".into(), (reported["published"] == Value::Bool(true)).into());
        m
    }
}

/// What GitHub said of a commit, read: green (`ci/green/<sha>`'s ref
/// answered 200), revoked (`ci/revoked/<sha>`'s did), and the compare of
/// it with `main` (`GET /repos/{repo}/compare/{sha}...main`: `identical`
/// or `ahead` when it's on `main`). `Err` why it can't be released.
pub fn source_verdict(sha: &str, green: u16, revoked: u16, compare: &Value) -> Result<(), String> {
    if revoked == 200 {
        return Err(format!("{sha} passed CI and then failed a re-run (ci/revoked): it isn't released"));
    }
    if green != 200 {
        return Err(format!("{sha} has no ci/green tag on GitHub: it isn't released"));
    }
    let on_main =
        matches!(compare["status"].as_str(), Some("identical" | "ahead")) && compare["behind_by"].as_u64() == Some(0);
    if !on_main {
        return Err(format!("{sha} isn't on {REPO}'s {BRANCH}: it isn't released"));
    }
    Ok(())
}

/// Before anything is minted for `d`'s run: GitHub, asked with the App's
/// read token, says the commit is on `main` and green (the box checks
/// again). `Err` why not.
pub async fn source_checked(app: &Platform, d: &Deploy) -> Result<(), String> {
    let sha = d.source.reference.as_str();
    if d.source.kind != SourceKind::Commit || d.source.place != REPO {
        return Err(format!("the {NAME} driver releases a commit of {REPO} only"));
    }
    let gh = app.github.as_ref().ok_or("this platform has no GitHub App to check the commit with")?;
    let install = match gh.installation(&app.http, REPO).await {
        Ok(Some(i)) => i,
        Ok(None) => return Err(format!("the platform's GitHub App isn't installed on {REPO}")),
        Err(e) => return Err(format!("couldn't ask GitHub about {REPO}: {e:#}")),
    };
    let caller = Caller::of(app, "platform");
    let token = gh
        .read_token(&caller, install.id, Repo::Name(REPO), Purpose::Read)
        .await
        .map_err(|e| format!("couldn't read {REPO}: {e:#}"))?;
    let api = gh.api.trim_end_matches('/');
    let get = async |path: String| -> Result<(u16, Value), String> {
        let r = app
            .http
            .get(format!("{api}{path}"))
            .bearer_auth(token.secret())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "arugula-platform")
            .send()
            .await
            .map_err(|e| format!("couldn't ask GitHub about {sha}: {e}"))?;
        let status = r.status().as_u16();
        Ok((status, r.json::<Value>().await.unwrap_or_default()))
    };
    let (green, _) = get(format!("/repos/{REPO}/git/ref/tags/ci/green/{sha}")).await?;
    let (revoked, _) = get(format!("/repos/{REPO}/git/ref/tags/ci/revoked/{sha}")).await?;
    let (status, compare) = get(format!("/repos/{REPO}/compare/{sha}...{BRANCH}")).await?;
    if status != 200 {
        return Err(format!("GitHub couldn't compare {sha} with {BRANCH} ({status}): it isn't released"));
    }
    source_verdict(sha, green, revoked, &compare)
}
