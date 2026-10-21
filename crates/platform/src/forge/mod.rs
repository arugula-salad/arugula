//! The GitHub App (M40), the platform's since M161 (#1207; "Control is two
//! processes", aru-075). The platform holds the App's id, private key and
//! webhook secret ([`Args`]); signing in with GitHub stays the hub's
//! (aru-078), with the App's client id and secret.
//!
//! - **Webhooks** ([`webhook`]): `POST /github/webhook` stays on control's
//!   site (aru-076), forwarded by the hub's gateway. Signature and dedupe
//!   are here; the pokes go to the hub, which relays them to daemons.
//! - **Who may hear about a repository** ([`standing`]): the account signed
//!   in to control with GitHub, the App is installed where the repository
//!   lives, and that installation is the person's own account, or GitHub
//!   says they're a collaborator on it ([`GithubApp::may`], kept by
//!   [`viewer::access`] in the database, which GitHub's events drop on
//!   every machine). Both by the
//!   person's numeric GitHub id. The hub asks it for each daemon's
//!   `forge.watch` and heartbeat (`PlatformPort::forge_standing`): it keeps
//!   the subscriptions, which live as long as a daemon's relay socket.
//! - **Hosted boxes** ([`token`]): the hub's `POST /api/daemon/github/token`
//!   asks for a read-only installation token for one repository
//!   (`PlatformPort::github_token`), for an account the same rule allows.
//!   The platform keeps a token until five minutes before GitHub's
//!   `expires_at`, never longer.
//! - **Tokens** (M167): one helper mints them all, each for one repository
//!   with the least it needs, and logs each mint ([`app`]'s docs).
//!
//! - **Linked repos** ([`links`], M163): a project's members who maintain
//!   a repo on GitHub link it to the project; the installation's events
//!   unlink it, and a purge takes an unlinked repo's rows within a day.
//! - **Reading items** ([`viewer`], [`items`], M165): every read of a
//!   linked repo's items checks the viewer's own access, and GitHub's
//!   events that change it drop what was kept.
//! - **The mirror** (M164): a linked repo's webhooks write its rows
//!   ([`ingest`]), and one leased poller per repo backfills and reconciles
//!   them ([`poller`]), within a rate budget; [`stats`] are its numbers.
//! - **Bodies and comments** ([`content`], M166): fetched on read and kept
//!   a few minutes in memory, or mirrored for a project that opts in.
//!
//! First-time lookups at GitHub are rate-limited per account
//! ([`FORGE_LOOKUPS`], counted here).

use std::sync::Arc;

use arugula_control_wire::forge::{GithubToken, Standing};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::Platform;

mod app;
pub mod content;
#[cfg(test)]
mod content_tests;
pub mod ingest;
#[cfg(test)]
mod ingest_tests;
pub mod items;
pub mod links;
#[cfg(test)]
mod links_tests;
pub mod poller;
pub mod stats;
pub mod viewer;
#[cfg(test)]
mod viewer_tests;
mod webhook;

pub use app::{
    Caller, GithubApp, GithubUser, Install, Purpose, ReadToken, Repo, RepoRole, WriteToken, Writes, epoch_of, parse_pem,
};
pub use webhook::{pokes, signed, webhook};

/// Repositories looked up at GitHub for the first time in a while (not
/// the heartbeat's re-checks), per account per hour: what a burst of
/// `forge.watch` messages costs the App's quota.
pub const FORGE_LOOKUPS: (&str, usize) = ("forge-lookup", 600);

/// What the platform says when it has no App: the hub, which knows
/// whether control should have one, logs it as a misconfiguration.
pub const NO_APP: &str = "this control has no GitHub App";

/// How often old webhook deliveries are pruned.
const PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

/// The GitHub App's flags, with the names they had on the hub, so a
/// deploy's secrets don't change: control takes them (`#[command(flatten)]`:
/// in process it hands the App to the platform; out of process the hub only
/// needs the id and slug), and so does the platform's own binary.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(id = "github_app")]
pub struct Args {
    /// The GitHub App (M40) whose webhooks come to /github/webhook and
    /// whose installation tokens let hosted boxes read: its id, slug,
    /// webhook secret and private key (a PEM file, or the PEM itself in
    /// GITHUB_APP_PRIVATE_KEY, as a Fly secret). Off without an id and a
    /// key.
    #[arg(long, env = "GITHUB_APP_ID")]
    pub github_app_id: Option<String>,
    #[arg(long, env = "GITHUB_APP_SLUG", default_value = "arugula-io")]
    pub github_app_slug: String,
    #[arg(long, env = "GITHUB_APP_WEBHOOK_SECRET", hide_env_values = true)]
    pub github_app_webhook_secret: Option<String>,
    #[arg(long, env = "GITHUB_APP_PRIVATE_KEY_FILE")]
    pub github_app_private_key_file: Option<std::path::PathBuf>,
    #[arg(long, env = "GITHUB_APP_PRIVATE_KEY", hide_env_values = true)]
    pub github_app_private_key: Option<String>,
    /// How often each linked repo's poller reconciles its rows with
    /// GitHub (M164), in minutes: 5 unless set.
    #[arg(long, env = "ARUGULA_FORGE_RECONCILE_MINUTES")]
    pub forge_reconcile_minutes: Option<u64>,
    /// The share of each installation's GitHub rate limit, in percent,
    /// that the poller leaves for reads on demand and sends (M164): 30
    /// unless set.
    #[arg(long, env = "ARUGULA_FORGE_RATE_RESERVE", value_parser = clap::value_parser!(u8).range(0..=100))]
    pub forge_rate_reserve: Option<u8>,
}

impl Args {
    /// The App's id, if one is set.
    pub fn id(&self) -> Option<&str> {
        self.github_app_id.as_deref().filter(|s| !s.is_empty())
    }

    /// Whether its key or webhook secret is set here: the hub's warning,
    /// out of process, that they're the platform's now.
    pub fn has_secrets(&self) -> bool {
        let set = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.is_empty());
        set(&self.github_app_private_key)
            || self.github_app_private_key_file.is_some()
            || set(&self.github_app_webhook_secret)
    }

    /// The App, if there's an id, with GitHub's API at `api`. An id with no
    /// key is refused.
    pub fn app(&self, api: &str) -> anyhow::Result<Option<GithubApp>> {
        use anyhow::Context as _;
        let Some(id) = self.id() else {
            // Loud: GitHub's webhooks forwarded here are refused (503) until
            // the App's secrets are set (docs/control-ops.md).
            tracing::warn!(
                "no GITHUB_APP_ID: no GitHub App here, so GitHub webhooks are refused and forge blocks poll"
            );
            return Ok(None);
        };
        let pem = match (&self.github_app_private_key, &self.github_app_private_key_file) {
            (Some(k), _) if !k.is_empty() => k.clone(),
            (_, Some(f)) => std::fs::read_to_string(f)
                .with_context(|| format!("reading GITHUB_APP_PRIVATE_KEY_FILE {}", f.display()))?,
            _ => anyhow::bail!(
                "GITHUB_APP_ID is set but there's no GITHUB_APP_PRIVATE_KEY or GITHUB_APP_PRIVATE_KEY_FILE"
            ),
        };
        let key = parse_pem(&pem)?;
        let secret = self.github_app_webhook_secret.clone().unwrap_or_default();
        if secret.is_empty() {
            tracing::warn!("no GITHUB_APP_WEBHOOK_SECRET: every GitHub webhook will be refused");
        }
        info!(app = id, slug = self.github_app_slug, "GitHub App");
        let mut app = GithubApp::new(id.to_owned(), self.github_app_slug.clone(), secret, api, key);
        let d = ingest::Settings::default();
        app.ingest.settings = ingest::Settings {
            every_ms: self.forge_reconcile_minutes.filter(|m| *m > 0).map_or(d.every_ms, |m| m * 60_000),
            reserve: self.forge_rate_reserve.unwrap_or(d.reserve),
            ..d
        };
        Ok(Some(app))
    }
}

/// `OWNER/NAME`, as GitHub spells them.
pub fn good_repo(r: &str) -> bool {
    let mut p = r.split('/');
    let ok = |s: Option<&str>| {
        s.is_some_and(|s| {
            !s.is_empty()
                && !s.starts_with('.')
                && s.len() <= 100
                && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    };
    ok(p.next()) && ok(p.next()) && p.next().is_none()
}

/// Where each repository stands for an account's daemon: live (pokes come)
/// or why not. `github`: the account's GitHub identity, if it signed in
/// with GitHub. `fresh`: a daemon just asked (not the heartbeat), so
/// lookups GitHub hasn't answered lately count against the account's
/// limit.
pub async fn standing(
    p: &Platform,
    account: &str,
    github: Option<&GithubUser>,
    repos: &[String],
    fresh: bool,
) -> Vec<Standing> {
    let mut out = Vec::new();
    for repo in repos {
        let why = match (&p.github, github) {
            (None, _) => Some(format!("{NO_APP}: the block polls")),
            (Some(_), None) => Some("sign in to control with GitHub for live updates from GitHub".to_owned()),
            (Some(gh), Some(u)) => match viewer::access(p, gh, account, u, repo, fresh).await {
                Ok(viewer::Access::Yes) => None,
                Ok(viewer::Access::No(why)) => Some(why),
                Ok(viewer::Access::Busy) => {
                    Some("too many repositories asked about lately: live updates resume within the hour".to_owned())
                }
                Err(e) => {
                    tracing::warn!(error = format!("{e:#}"), "checking access to a repository");
                    Some("couldn't check your access to it".to_owned())
                }
            },
        };
        out.push(Standing { repo: repo.clone(), live: why.is_none(), why });
    }
    out
}

/// What the platform answers a hosted box's ask for a token: one, or why
/// not, as the hub's route answers it. No `Debug`: a token is never
/// printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAnswer {
    Token(GithubToken),
    Refused { status: u16, error: String },
}

/// A read-only installation token for `repo`, for an account GitHub gives
/// access to it ([`viewer::access`]: kept answers every machine drops).
pub async fn token(
    p: &Platform,
    account: &str,
    github: Option<&GithubUser>,
    repo: &str,
) -> anyhow::Result<TokenAnswer> {
    let refused =
        |status: StatusCode, error: &str| Ok(TokenAnswer::Refused { status: status.as_u16(), error: error.into() });
    let Some(gh) = &p.github else { return refused(StatusCode::NOT_FOUND, NO_APP) };
    if !good_repo(repo) {
        return refused(StatusCode::BAD_REQUEST, "repo: OWNER/NAME");
    }
    let Some(user) = github else {
        return refused(StatusCode::FORBIDDEN, "sign in to control with GitHub to read GitHub through its App");
    };
    match viewer::access(p, gh, account, user, repo, true).await? {
        viewer::Access::Yes => {}
        viewer::Access::No(why) => return refused(StatusCode::FORBIDDEN, &why),
        viewer::Access::Busy => {
            return refused(
                StatusCode::TOO_MANY_REQUESTS,
                "too many repositories asked about lately: try within the hour",
            );
        }
    }
    let caller = Caller::of(p, account);
    let Some(install) = gh.installation(&p.http, repo).await? else {
        return refused(StatusCode::FORBIDDEN, &format!("the GitHub App {} isn't installed for {repo}", gh.slug));
    };
    // It leaves the platform: logged for this account even from the cache.
    let t = gh.hand_out(&caller, install.id, Repo::Name(repo)).await?;
    // Who and what; never the token.
    info!(account, repo, "installation token for a daemon");
    Ok(TokenAnswer::Token(GithubToken {
        token: t.secret().to_owned(),
        expires_at: t.expires_at,
        login: user.login.clone(),
        app: gh.slug.clone(),
    }))
}

/// The forge's background work: unlinked repos' rows are purged
/// ([`links::start`], with or without the App: deleting an account
/// unlinks too), linked repos are polled ([`poller::start`]), webhook
/// deliveries older than a week go, the token log's mints older than a
/// year, and access answers no longer used.
pub fn start(p: &Arc<Platform>) {
    links::start(p);
    if p.github.is_none() {
        return;
    }
    poller::start(p);
    let p = p.clone();
    tokio::spawn(async move {
        loop {
            if !p.is_paused() {
                match p.db.prune_deliveries(arugula_e2e::now_ms()) {
                    Ok(n) if n > 0 => info!(deliveries = n, "pruned old GitHub webhook deliveries"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "pruning GitHub webhook deliveries"),
                }
                match p.db.prune_mints(arugula_e2e::now_ms()) {
                    Ok(n) if n > 0 => info!(mints = n, "pruned old GitHub token mints"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "pruning GitHub token mints"),
                }
                if let Err(e) = p.db.prune_access(viewer::KEEP_MS) {
                    tracing::warn!(error = %e, "pruning kept GitHub access answers");
                }
            }
            tokio::time::sleep(PRUNE_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos() {
        assert!(good_repo("jhgaylor/illogical"));
        assert!(good_repo("a.b/c-d_e"));
        assert!(!good_repo("a/b/c"));
        assert!(!good_repo("a"));
        assert!(!good_repo("a/b?x"));
        assert!(!good_repo("../x"));
    }

    #[test]
    fn token_answers_on_the_wire() {
        let t = TokenAnswer::Refused { status: 403, error: "no".into() };
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({ "refused": { "status": 403, "error": "no" } })
        );
        let t = TokenAnswer::Token(GithubToken { token: "t".into(), ..Default::default() });
        let back: TokenAnswer = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert!(back == t);
    }
}
