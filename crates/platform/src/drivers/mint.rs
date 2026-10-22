//! Short-lived tokens a driver mints at apply (M219): from a long-lived
//! credential the platform keeps, so the box gets only the short one.
//!
//! The first kind is a GitHub App installation token for the deploy's
//! source repository: minted fresh for the run (never the read cache),
//! scoped to that one repository's contents, read-only, logged in
//! `github_token_mints` (`deploy`), and revoked at GitHub when the run ends
//! by the box itself when its driver ends (the token as its own bearer),
//! and by the platform ([`revoke_run`], tried a few times); it lasts an
//! hour at most if both miss. The App's private key never leaves the platform. A Fly deploy
//! token scoped to one org (M220's chant→Fly driver) is the next kind: it
//! joins [`Mint`] there.

use anyhow::{Context as _, Result, bail};
use arugula_control_wire::work::Secret;

use crate::{
    Platform,
    db::deploys::{Deploy, SourceKind},
    forge::{Caller, Repo},
};

/// A token a driver mints at apply, and the name the run gets it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mint {
    /// A read-only installation token for the deploy's source repository
    /// (`owner/name`, a commit's `where`), through the platform's GitHub
    /// App.
    GithubRead { name: &'static str },
}

impl Mint {
    /// The name the run gets it under.
    pub fn name(&self) -> &'static str {
        match self {
            Mint::GithubRead { name } => name,
        }
    }
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
            app.deploy_tokens
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(run.to_owned())
                .or_default()
                .push(token.clone());
            Ok(Secret { name: name.to_owned(), value: token, revoke: Some(gh.revoke_url()) })
        }
    }
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
