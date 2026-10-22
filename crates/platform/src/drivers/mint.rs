//! Short-lived tokens a driver mints at apply (M219): from a long-lived
//! credential the platform keeps, so the box gets only the short one.
//!
//! The first kind is a GitHub App installation token for the deploy's
//! source repository: minted fresh for the run (never the read cache),
//! scoped to that one repository's contents, read-only, logged in
//! `github_token_mints` (`deploy`), and revoked at GitHub when the run ends
//! by the box itself when its driver ends (the token as its own bearer),
//! and by the platform ([`revoke_run`], tried a few times); it lasts an
//! hour at most if both miss. The App's private key never leaves the
//! platform. The second (M222's apps-platform driver) writes: an
//! installation token for one named repository (the driver's list),
//! `contents: write` and nothing more, minted, logged (`deploy:contents`)
//! and revoked the same way. M220's chant→Fly driver mints the same kind
//! for the deploy's own repository ([`Mint::GithubWriteSource`]): its box
//! pushes chant's ledger there.
//!
//! A deploy whose run would mint a token that writes is refused when it's
//! planned, not when it runs, if the App can't: not installed on the
//! repository, or installed without `contents: write`
//! ([`plan_refuses`]). An owner never approves a deploy that can't run.

use anyhow::{Context as _, Result, bail};
use arugula_control_wire::work::Secret;

use crate::{
    Platform,
    db::deploys::{Deploy, Environment, Source, SourceKind},
    forge::{Caller, Repo},
};

/// A token a driver mints at apply, and the name the run gets it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mint {
    /// A read-only installation token for the deploy's source repository
    /// (`owner/name`, a commit's `where`), through the platform's GitHub
    /// App.
    GithubRead { name: &'static str },
    /// An installation token that writes `repo`'s contents (`owner/name`)
    /// and nothing else, through the platform's GitHub App.
    GithubWrite { name: &'static str, repo: &'static str },
    /// An installation token that writes the deploy's source repository's
    /// contents (a commit's `where`) and nothing else.
    GithubWriteSource { name: &'static str },
}

impl Mint {
    /// The name the run gets it under.
    pub fn name(&self) -> &'static str {
        match self {
            Mint::GithubRead { name } | Mint::GithubWrite { name, .. } | Mint::GithubWriteSource { name } => name,
        }
    }

    /// The repository it writes for deploys of `source` (`owner/name`):
    /// `None` when it only reads.
    pub fn writes<'a>(&self, source: &'a Source) -> Option<&'a str> {
        match self {
            Mint::GithubRead { .. } => None,
            Mint::GithubWrite { repo, .. } => Some(repo),
            Mint::GithubWriteSource { .. } => Some(&source.place),
        }
    }
}

/// Why `env`'s driver couldn't mint a token that writes for a deploy of
/// `source`, said when it's planned: `None` when it could. GitHub says what
/// the App's installation on the repository grants
/// (`GET /repos/{owner}/{repo}/installation`'s `permissions`).
pub async fn plan_refuses(app: &Platform, env: &Environment, source: &Source) -> Option<String> {
    let driver = super::driver(&env.driver)?;
    for m in driver.mints(env) {
        let Some(repo) = m.writes(source) else { continue };
        let name = m.name();
        let own = format!("or give the environment a {name} of its own");
        let Some(gh) = app.github.as_ref() else {
            return Some(format!("this platform has no GitHub App to mint {name} with: {own}"));
        };
        if source.kind != SourceKind::Commit && matches!(m, Mint::GithubWriteSource { .. }) {
            return Some(format!("{name} is minted only for a commit's repository"));
        }
        match gh.installation(&app.http, repo).await {
            Ok(Some(i)) if i.grants("contents", "write") => {}
            Ok(Some(_)) => {
                return Some(format!(
                    "the platform's GitHub App can't write {repo}'s contents (its installation doesn't grant \
                     Contents: write), so the {name} it mints couldn't push: grant it that, {own}"
                ));
            }
            Ok(None) => {
                return Some(format!(
                    "the platform's GitHub App isn't installed on {repo}, so it can't mint {name}: install it, {own}"
                ));
            }
            Err(e) => return Some(format!("couldn't ask GitHub what the App may do on {repo}: {e:#}")),
        }
    }
    None
}

/// Tokens minted for runs, by run, to revoke when each ends: in memory
/// only (a token is never written down), so a restart leaves them to
/// expire within the hour.
#[derive(Default)]
pub struct Held(std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>);

/// Mint `m` for deploy `d`'s run `run`, now.
pub async fn mint(app: &Platform, m: Mint, d: &Deploy, run: &str) -> Result<Secret> {
    match m {
        Mint::GithubRead { name } => {
            if d.source.kind != SourceKind::Commit {
                bail!("a GitHub token is minted only for a commit's repository");
            }
            let gh = app.github.as_ref().context("this platform has no GitHub App to mint a token with")?;
            let repo = d.source.place.as_str();
            let install = gh
                .installation(&app.http, repo)
                .await?
                .with_context(|| format!("the GitHub App isn't installed on {repo}"))?;
            let caller = Caller::of(app, "platform");
            let (token, _) = gh.deploy_token(&caller, install.id, Repo::Name(repo)).await?;
            Ok(held(app, run, name, token, gh.revoke_url()))
        }
        Mint::GithubWrite { name, repo } => write(app, name, repo, run).await,
        Mint::GithubWriteSource { name } => {
            if d.source.kind != SourceKind::Commit {
                bail!("a GitHub token is minted only for a commit's repository");
            }
            write(app, name, &d.source.place, run).await
        }
    }
}

/// An installation token that writes `repo`'s contents, for `run`, as
/// `name`.
async fn write(app: &Platform, name: &str, repo: &str, run: &str) -> Result<Secret> {
    let gh = app.github.as_ref().context("this platform has no GitHub App to mint a token with")?;
    let install =
        gh.installation(&app.http, repo).await?.with_context(|| format!("the GitHub App isn't installed on {repo}"))?;
    let caller = Caller::of(app, "platform");
    let (token, _) = gh.deploy_write_token(&caller, install.id, Repo::Name(repo)).await?;
    Ok(held(app, run, name, token, gh.revoke_url()))
}

/// Keep `token` to revoke when `run` ends, and hand it over as `name`,
/// with where the box revokes it itself (`revoke`).
fn held(app: &Platform, run: &str, name: &str, token: String, revoke: String) -> Secret {
    app.deploy_tokens
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(run.to_owned())
        .or_default()
        .push(token.clone());
    Secret { name: name.to_owned(), value: token, revoke: Some(revoke) }
}

/// Revoke what was minted for `run`: its run ended.
pub async fn revoke_run(app: &Platform, run: &str) {
    let tokens = app.deploy_tokens.0.lock().unwrap_or_else(|e| e.into_inner()).remove(run).unwrap_or_default();
    // The box revokes them itself when its driver ends; this is the
    // platform's own try, a few times over.
    if let Some(gh) = app.github.as_ref() {
        for t in &tokens {
            for attempt in 0..3u64 {
                if gh.revoke_token(&app.http, t).await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500 * (attempt + 1))).await;
            }
        }
    }
    if !tokens.is_empty() {
        tracing::info!(run, tokens = tokens.len(), "revoked a deploy run's minted tokens");
    }
}
