//! M40: daemons' forge subscriptions and the pokes they hear, the hub's
//! until M174 retires the relay. The GitHub App itself (its webhook, key,
//! `may()` and installation tokens) is the platform's since M161
//! (`arugula_platform::forge`); the hub asks it over `PlatformPort`.
//!
//! - **Subscriptions.** A daemon with GitHub forge blocks open says which
//!   repositories over its relay socket (a text message
//!   `{"t": "forge.watch", "repos": [...]}`); control answers, and every
//!   minute after (the heartbeat), with `{"t": "forge.watching", "repos":
//!   [{repo, live, why?}]}`. Pokes go only to daemons whose account may see
//!   the repository, as the platform says (`PlatformPort::forge_standing`).
//!   Subscriptions live as long as the socket. A daemon watches at most 200
//!   repositories and an account 400; `forge.watch` messages are
//!   rate-limited per daemon (limit.rs), first-time lookups at GitHub per
//!   account (by the platform).
//! - **Pokes.** The platform checks and dedupes the App's webhooks and
//!   hands their pokes here (`HubPort::forge_pokes`, [`relay_pokes`]):
//!   each goes to the daemons watching its repository as a `forge.poke`
//!   message, the same bytes as ever, so old daemons hear them unchanged.
//! - **Hosted boxes.** `POST /api/daemon/github/token {repo}` (a daemon's
//!   signature): the platform's read-only installation token for one
//!   repository (`PlatformPort::github_token`), for an account the same
//!   rule allows.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::forge::{
    ForgePoke, ForgeWatch, ForgeWatching, GithubToken, GithubTokenRequest, Poke, Standing,
};
use arugula_platform::{
    PlatformPort as _,
    forge::{GithubUser, NO_APP, TokenAnswer, good_repo},
};
use axum::{Json, extract::State, http::StatusCode};
use tracing::{info, warn};

use crate::{ApiError, App, auth::DaemonAuth, err};

/// Repositories one daemon may watch.
const MAX_REPOS: usize = 200;
/// Repositories one account's daemons may watch, together.
const MAX_ACCOUNT_REPOS: usize = 400;

struct Watch {
    generation: u64,
    account: String,
    repos: Vec<String>,
    standing: BTreeMap<String, Standing>,
}

/// Daemons' subscriptions.
#[derive(Default)]
pub struct Watches {
    by_daemon: Mutex<HashMap<String, Watch>>,
}

impl Watches {
    /// The daemons a poke for `repo` goes to.
    pub fn route(&self, repo: &str) -> Vec<String> {
        let k = repo.to_ascii_lowercase();
        let mut out: Vec<String> = self
            .by_daemon
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, w)| w.standing.get(&k).is_some_and(|s| s.live))
            .map(|(d, _)| d.clone())
            .collect();
        out.sort();
        out
    }

    /// A daemon's socket closed: its subscription goes with it.
    pub fn drop_daemon(&self, daemon: &str, generation: u64) {
        let mut m = self.by_daemon.lock().unwrap();
        if m.get(daemon).is_some_and(|w| w.generation == generation) {
            m.remove(daemon);
        }
    }

    fn set(&self, daemon: &str, generation: u64, account: &str, repos: Vec<String>, standing: Vec<Standing>) {
        let standing = standing.into_iter().map(|s| (s.repo.to_ascii_lowercase(), s)).collect();
        self.by_daemon
            .lock()
            .unwrap()
            .insert(daemon.to_owned(), Watch { generation, account: account.to_owned(), repos, standing });
    }

    /// Repositories (lowercase) the account's other daemons watch.
    fn account_repos(&self, account: &str, except: &str) -> HashSet<String> {
        self.by_daemon
            .lock()
            .unwrap()
            .iter()
            .filter(|(d, w)| w.account == account && d.as_str() != except)
            .flat_map(|(_, w)| w.repos.iter().map(|r| r.to_ascii_lowercase()))
            .collect()
    }

    fn snapshot(&self) -> Vec<(String, u64, String, Vec<String>)> {
        self.by_daemon
            .lock()
            .unwrap()
            .iter()
            .map(|(d, w)| (d.clone(), w.generation, w.account.clone(), w.repos.clone()))
            .collect()
    }
}

/// The account's GitHub identity, if it signed in with GitHub.
fn github_user(app: &App, account: &str) -> anyhow::Result<Option<GithubUser>> {
    Ok(app.db.github_identity(account)?.and_then(|(id, login)| Some(GithubUser { id: id.parse().ok()?, login })))
}

/// Where each repository stands for an account, as the platform says
/// (M161: the App's `may()` is the platform's). `fresh`: a daemon just
/// asked (not the heartbeat).
async fn standings(app: &App, account: &str, repos: &[String], fresh: bool) -> Vec<Standing> {
    let not =
        |why: &str| repos.iter().map(|r| Standing { repo: r.clone(), live: false, why: Some(why.into()) }).collect();
    if app.github_app.is_none() {
        return not(&format!("{NO_APP}: the block polls"));
    }
    let user = github_user(app, account).ok().flatten();
    match app.platform.forge_standing(account, user.as_ref(), repos, fresh).await {
        Ok(st) => {
            if st.iter().any(|s| s.why.as_deref().is_some_and(|w| w.starts_with(NO_APP))) {
                platform_has_no_app();
            }
            st
        }
        Err(e) => {
            warn!(error = format!("{e:#}"), "asking the platform where repositories stand");
            not("couldn't reach control's platform: live updates resume when it's back")
        }
    }
}

/// What a daemon says over its relay socket (anything but "trust").
pub fn from_daemon(app: &Arc<App>, daemon: &str, generation: u64, text: &str) {
    // Another `t` doesn't parse as a watch: not ours, passed over.
    let Ok(m) = serde_json::from_str::<ForgeWatch>(text) else { return };
    if app.limits.check_daemon(crate::limit::FORGE_WATCHES, daemon).is_err() {
        warn!(%daemon, "too many forge.watch messages; ignoring this one");
        return;
    }
    let mut repos: Vec<String> = m.repos.into_iter().filter(|r| good_repo(r)).take(MAX_REPOS).collect();
    repos.sort_by_key(|r| r.to_ascii_lowercase());
    repos.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let (app, daemon) = (app.clone(), daemon.to_owned());
    tokio::spawn(async move {
        let Ok(Some(account)) = app.db.daemon_account(&daemon) else { return };
        // The account's machines together watch at most so many.
        let mut others = app.forge.account_repos(&account, &daemon);
        let (ok, over): (Vec<String>, Vec<String>) = repos.into_iter().partition(|r| {
            let k = r.to_ascii_lowercase();
            others.contains(&k) || (others.len() < MAX_ACCOUNT_REPOS && others.insert(k))
        });
        let repos = ok;
        let mut st = standings(&app, &account, &repos, true).await;
        st.extend(over.into_iter().map(|repo| Standing {
            repo,
            live: false,
            why: Some(format!("this account's machines watch {MAX_ACCOUNT_REPOS} repositories already")),
        }));
        let live = st.iter().filter(|s| s.live).count();
        info!(%daemon, repos = repos.len(), live, "forge subscriptions");
        app.forge.set(&daemon, generation, &account, repos, st.clone());
        app.relay.text(&daemon, &watching(&st));
    });
}

fn watching(st: &[Standing]) -> String {
    serde_json::to_string(&ForgeWatching::new(st.to_vec())).unwrap_or_default()
}

/// Every `every`, each subscribed daemon hears where its repositories
/// stand (looked at again when the answers are old): the heartbeat that
/// says the webhook path is up.
pub async fn heartbeat(app: Arc<App>, every: Duration) {
    loop {
        tokio::time::sleep(every).await;
        for (daemon, generation, account, repos) in app.forge.snapshot() {
            let st = standings(&app, &account, &repos, false).await;
            app.forge.set(&daemon, generation, &account, repos, st.clone());
            app.relay.text(&daemon, &watching(&st));
        }
    }
}

/// This hub has an App (`GITHUB_APP_ID`) and the platform says it has
/// none: its secrets weren't set (M161, docs/control-ops.md).
fn platform_has_no_app() {
    tracing::error!(
        "GITHUB_APP_ID is set here, and the platform has no GitHub App: set the App's secrets on the platform \
         (GITHUB_APP_ID, GITHUB_APP_SLUG, GITHUB_APP_WEBHOOK_SECRET, GITHUB_APP_PRIVATE_KEY)"
    );
}

/// The platform's pokes for a webhook (`HubPort::forge_pokes`): each to
/// the daemons watching its repository, where the platform said it's
/// live. How many messages went.
pub fn relay_pokes(app: &App, pokes: &[Poke]) -> u32 {
    let mut relayed = 0;
    for p in pokes {
        let msg = serde_json::to_string(&ForgePoke::new(p.clone())).unwrap_or_default();
        let to = app.forge.route(&p.repo);
        for d in &to {
            app.relay.text(d, &msg);
        }
        relayed += to.len() as u32;
        info!(
            event = p.event,
            delivery = p.delivery,
            repo = p.repo,
            number = p.number,
            daemons = to.len(),
            "webhook relayed"
        );
    }
    relayed
}

/// `POST /api/daemon/github/token {repo}`: a hosted box (no `gh` login)
/// reads through the App, for repositories its account may. The platform
/// holds the App's key and answers (M161).
pub async fn daemon_token(
    State(app): State<Arc<App>>,
    d: DaemonAuth,
    Json(b): Json<GithubTokenRequest>,
) -> Result<Json<GithubToken>, ApiError> {
    if app.github_app.is_none() {
        return Err(err(StatusCode::NOT_FOUND, NO_APP));
    }
    if !good_repo(&b.repo) {
        return Err(err(StatusCode::BAD_REQUEST, "repo: OWNER/NAME"));
    }
    let user = github_user(&app, &d.cert.account)?;
    match app.platform.github_token(&d.cert.account, user.as_ref(), &b.repo).await? {
        TokenAnswer::Token(t) => {
            // Who and what; never the token.
            info!(daemon = d.cert.device, repo = b.repo, "installation token for a daemon");
            Ok(Json(t))
        }
        TokenAnswer::Refused { status, error } => {
            if error == NO_APP {
                platform_has_no_app();
            }
            Err(err(StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN), &error))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use aws_lc_rs::{
        encoding::AsDer,
        rsa::{KeyPair, KeySize},
        signature::KeyPair as _,
    };
    use base64::Engine as _;

    use super::*;

    fn pem(label: &str, der: &[u8]) -> String {
        let b = base64::engine::general_purpose::STANDARD.encode(der);
        let lines: Vec<String> = b.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        format!("-----BEGIN {label}-----\n{}\n-----END {label}-----\n", lines.join("\n"))
    }

    pub(crate) fn throwaway_key() -> (String, Vec<u8>) {
        let k = KeyPair::generate(KeySize::Rsa2048).unwrap();
        let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
        (pem("PRIVATE KEY", der.as_ref()), k.public_key().as_ref().to_vec())
    }

    #[test]
    fn routing_only_where_live() {
        let w = Watches::default();
        let st = |repo: &str, live| Standing { repo: repo.into(), live, why: None };
        w.set("d1", 1, "a1", vec!["O/R".into()], vec![st("O/R", true), st("o/x", false)]);
        w.set("d2", 1, "a2", vec!["o/r".into()], vec![st("o/r", false)]);
        w.set("d3", 4, "a1", vec!["o/r".into()], vec![st("o/r", true)]);
        assert_eq!(w.route("o/r"), vec!["d1", "d3"]);
        assert!(w.route("o/x").is_empty());
        // An old socket's end leaves the new one's subscription.
        w.drop_daemon("d3", 3);
        assert_eq!(w.route("o/r"), vec!["d1", "d3"]);
        w.drop_daemon("d3", 4);
        assert_eq!(w.route("o/r"), vec!["d1"]);
    }
}
