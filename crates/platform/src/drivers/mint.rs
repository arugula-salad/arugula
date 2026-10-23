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
//! **A token for the deploy's own repository** (read or write) is minted
//! only when that repository is linked to the deploy's project, live, and
//! for it by GitHub's id ([`source_link`]): the App is installed on many
//! repositories, and a deploy's `where` is the planner's to say. Checked
//! when it's planned and again when it's minted, since a repository may
//! be unlinked after the approval.
//!
//! A deploy whose run would mint a token that writes is refused when it's
//! planned, not when it runs, if the App can't: not installed on the
//! repository, or installed without `contents: write`
//! ([`plan_refuses`]). An owner never approves a deploy that can't run.
//!
//! **planter's drivers (M233) mint from planter's App**, the deploy App
//! (aru-108), never the platform's ([`Mint::Planter`]): one repository each,
//! with the scope the driver needs ([`DeployScope`]), checked against that
//! App's installation when the deploy is planned, logged as
//! [`PLANTER_CALLER`]'s, and revoked the same way. The deploy's own
//! repository is still minted for only while it's linked to the project.

use anyhow::{Context as _, Result};
use arugula_control_wire::work::Secret;

use crate::{
    Platform,
    db::{
        deploys::{Deploy, Environment, Source, SourceKind},
        forge::LinkedRepo,
    },
    forge::{Caller, DeployScope, Repo},
};

/// Which repository a token of planter's App is for (M233).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanterRepo {
    /// One named repository (`owner/name`): the pointers, `shipped`.
    Named(&'static str),
    /// The deploy's own source repository, which must be linked to the
    /// deploy's project ([`source_link`]).
    Source,
}

/// What the mints of planter's App are logged as having been for: the
/// caller, so the log tells them from the platform's own App's.
pub const PLANTER_CALLER: &str = "planter-app";

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
    /// An installation token that reads `repo`'s Actions runs and contents
    /// (M221: following a release's workflow), and writes nothing.
    GithubFollow { name: &'static str, repo: &'static str },
    /// An installation token of planter's GitHub App, the deploy App (M233,
    /// aru-108), for one repository with `scope`: planter's drivers' runs
    /// get only these, never one of the platform's own App.
    Planter { name: &'static str, repo: PlanterRepo, scope: DeployScope },
}

impl Mint {
    /// The name the run gets it under.
    pub fn name(&self) -> &'static str {
        match self {
            Mint::GithubRead { name }
            | Mint::GithubWrite { name, .. }
            | Mint::GithubWriteSource { name }
            | Mint::GithubFollow { name, .. }
            | Mint::Planter { name, .. } => name,
        }
    }

    /// Whether it's for the deploy's own repository, which must be linked
    /// to the deploy's project ([`source_link`]).
    pub fn for_source(&self) -> bool {
        matches!(
            self,
            Mint::GithubRead { .. } | Mint::GithubWriteSource { .. } | Mint::Planter { repo: PlanterRepo::Source, .. }
        )
    }

    /// Whether it's minted from planter's App, the deploy App (M233).
    pub fn planters(&self) -> bool {
        matches!(self, Mint::Planter { .. })
    }

    /// The repository it's for, for deploys of `source`.
    pub fn repo<'a>(&self, source: &'a Source) -> &'a str {
        match self {
            Mint::GithubWrite { repo, .. }
            | Mint::GithubFollow { repo, .. }
            | Mint::Planter { repo: PlanterRepo::Named(repo), .. } => repo,
            _ => &source.place,
        }
    }

    /// The repository it writes for deploys of `source` (`owner/name`):
    /// `None` when it only reads.
    pub fn writes<'a>(&self, source: &'a Source) -> Option<&'a str> {
        match self {
            Mint::GithubRead { .. } | Mint::GithubFollow { .. } => None,
            Mint::GithubWrite { repo, .. } => Some(repo),
            Mint::GithubWriteSource { .. } => Some(&source.place),
            Mint::Planter { scope, .. } => (!scope.writes().is_empty()).then(|| self.repo(source)),
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
        let name = m.name();
        let mut linked = None;
        if m.for_source() {
            match source_link(app, &env.project, source) {
                Ok(Some(l)) => linked = Some(l.full_name),
                Ok(None) => return Some(not_linked(name, source)),
                Err(e) => return Some(format!("couldn't read the project's repositories: {e:#}")),
            }
        }
        // planter's App (M233): installed on the repository, granting each
        // permission the token writes, or the run couldn't have it.
        if let Mint::Planter { scope, .. } = m {
            let repo = if m.for_source() { linked.as_deref().unwrap_or(&source.place) } else { m.repo(source) };
            if let Some(why) = planter_refuses(app, name, repo, scope).await {
                return Some(why);
            }
            continue;
        }
        // The repository as it's linked (GitHub's spelling), for one of
        // the source's.
        let Some(repo) = m.writes(source).map(|r| if m.for_source() { linked.as_deref().unwrap_or(r) } else { r })
        else {
            continue;
        };
        let own = format!("or give the environment a {name} of its own");
        let Some(gh) = app.github.as_ref() else {
            return Some(format!("this platform has no GitHub App to mint {name} with: {own}"));
        };
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

/// Why planter's App couldn't mint `name` for `repo` with `scope`: `None`
/// when it could. Never the platform's own App: it isn't asked.
async fn planter_refuses(app: &Platform, name: &str, repo: &str, scope: DeployScope) -> Option<String> {
    let Some(gh) = app.deploy_app.as_ref() else {
        return Some(format!(
            "this platform has no planter GitHub App (PLANTER_APP_ID) to mint {name} with: planter's drivers deploy \
             through planter's App only"
        ));
    };
    match gh.installation(&app.http, repo).await {
        Ok(Some(i)) => match scope.writes().iter().find(|p| !i.grants(p, "write")) {
            None if i.grants("contents", "read") => None,
            None => Some(format!("planter's GitHub App ({}) can't read {repo}'s contents", gh.slug)),
            Some(p) => Some(format!(
                "planter's GitHub App ({}) can't write {repo}'s {p} (its installation doesn't grant it), so the {name} \
                 it mints couldn't do what the plan says: grant it that",
                gh.slug
            )),
        },
        Ok(None) => Some(format!(
            "planter's GitHub App ({}) isn't installed on {repo}, so it can't mint {name}: install it there",
            gh.slug
        )),
        Err(e) => Some(format!("couldn't ask GitHub what planter's App may do on {repo}: {e:#}")),
    }
}

/// The deploy's source repository as `project` links it now (not
/// unlinked), by its name in any case: `None` when it isn't, or the source
/// isn't a commit's repository.
pub fn source_link(app: &Platform, project: &str, source: &Source) -> Result<Option<LinkedRepo>> {
    if source.kind != SourceKind::Commit {
        return Ok(None);
    }
    Ok(app.db.linked_repo_named(&source.place)?.filter(|l| l.project_id == project && l.unlinked_at.is_none()))
}

fn not_linked(name: &str, source: &Source) -> String {
    format!(
        "{} isn't a repository linked to this project, so its run can't have a {name} for it: link it to the \
         project first",
        source.place
    )
}

/// Tokens minted for runs, by run, to revoke when each ends: in memory
/// only (a token is never written down), so a restart leaves them to
/// expire within the hour.
#[derive(Default)]
pub struct Held(std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>);

/// Mint `m` for deploy `d` (of `project`)'s run `run`, now. A token for
/// the deploy's own repository is minted for it by GitHub's id, and only
/// while it's linked to `project`.
pub async fn mint(app: &Platform, m: Mint, d: &Deploy, project: &str, run: &str) -> Result<Secret> {
    if let Mint::Planter { scope, .. } = m {
        return mint_planters(app, m, scope, d, project, run).await;
    }
    let gh = app.github.as_ref().context("this platform has no GitHub App to mint a token with")?;
    let caller = Caller::of(app, "platform");
    let token = match m {
        Mint::GithubRead { .. } | Mint::GithubWriteSource { .. } => {
            let link = source_link(app, project, &d.source)?.with_context(|| not_linked(m.name(), &d.source))?;
            let repo = Repo::Id(link.repo_id);
            if matches!(m, Mint::GithubRead { .. }) {
                gh.deploy_token(&caller, link.installation_id, repo).await?.0
            } else {
                gh.deploy_write_token(&caller, link.installation_id, repo).await?.0
            }
        }
        Mint::GithubWrite { repo, .. } => {
            let install = gh
                .installation(&app.http, repo)
                .await?
                .with_context(|| format!("the GitHub App isn't installed on {repo}"))?;
            gh.deploy_write_token(&caller, install.id, Repo::Name(repo)).await?.0
        }
        Mint::GithubFollow { repo, .. } => {
            let install = gh
                .installation(&app.http, repo)
                .await?
                .with_context(|| format!("the GitHub App isn't installed on {repo}"))?;
            gh.deploy_follow_token(&caller, install.id, Repo::Name(repo)).await?.0
        }
        Mint::Planter { .. } => unreachable!("minted by planter's App above"),
    };
    Ok(held(app, run, m.name(), token, gh.revoke_url()))
}

/// Mint `m`, one of planter's drivers' tokens, from planter's App (M233):
/// for the deploy's own repository by GitHub's id, only while it's linked
/// to `project`; for a named one by its name. Logged as
/// [`PLANTER_CALLER`]'s, revoked when the run ends as every run's are.
async fn mint_planters(
    app: &Platform,
    m: Mint,
    scope: DeployScope,
    d: &Deploy,
    project: &str,
    run: &str,
) -> Result<Secret> {
    let gh = app.deploy_app.as_ref().context("this platform has no planter GitHub App (PLANTER_APP_ID)")?;
    let caller = Caller::of(app, PLANTER_CALLER);
    let (named, repo) = if m.for_source() {
        let link = source_link(app, project, &d.source)?.with_context(|| not_linked(m.name(), &d.source))?;
        (link.full_name, Repo::Id(link.repo_id))
    } else {
        let r = m.repo(&d.source);
        (r.to_owned(), Repo::Name(r))
    };
    let install = gh
        .installation(&app.http, &named)
        .await?
        .with_context(|| format!("planter's GitHub App isn't installed on {named}"))?;
    let token = gh.deploy_scoped(&caller, install.id, repo, scope).await?.0;
    Ok(held(app, run, m.name(), token, gh.revoke_url()))
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
    // Revoking needs only GitHub's API (the token is its own bearer): either
    // App's handle says where it is.
    if let Some(gh) = app.github.as_ref().or(app.deploy_app.as_ref()) {
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
