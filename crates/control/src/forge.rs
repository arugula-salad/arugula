//! M40's GitHub App, as much of it as the hub keeps. The App itself (its
//! webhook, key, `may()` and installation tokens) is the platform's since
//! M161 (`arugula_platform::forge`); the hub asks it over `PlatformPort`.
//!
//! - **Subscriptions, retired (M174).** Daemons before 0.57 say which
//!   repositories they have GitHub blocks on over their relay socket
//!   (`{"t": "forge.watch", "repos": [...]}`), and the hub relayed the
//!   App's webhooks to them as pokes. The relay is gone: every
//!   `forge.watch` is answered with an empty `{"t": "forge.watching",
//!   "repos": []}`, so no old daemon errors. A daemon told nothing is live
//!   on nothing, and its blocks poll GitHub with its own login. Linked
//!   repos' blocks are pins onto the platform's mirror (M172).
//! - **Hosted boxes.** `POST /api/daemon/github/token {repo}` (a daemon's
//!   signature): the platform's read-only installation token for one
//!   repository (`PlatformPort::github_token`), for an account GitHub
//!   gives access to it.

use std::sync::Arc;

use arugula_control_wire::forge::{ForgeWatch, ForgeWatching, GithubToken, GithubTokenRequest};
use arugula_platform::{
    PlatformPort as _,
    forge::{GithubUser, NO_APP, TokenAnswer, good_repo},
};
use axum::{Json, extract::State, http::StatusCode};
use tracing::info;

use crate::{ApiError, App, auth::DaemonAuth, err};

/// The account's GitHub identity, if it signed in with GitHub.
pub(crate) fn github_user(app: &App, account: &str) -> anyhow::Result<Option<GithubUser>> {
    Ok(app.db.github_identity(account)?.and_then(|(id, login)| Some(GithubUser { id: id.parse().ok()?, login })))
}

/// What a daemon says over its relay socket (anything but "trust"). A
/// `forge.watch` hears an empty `forge.watching` (M174): no repository is
/// live, so its blocks poll.
pub fn from_daemon(app: &App, daemon: &str, text: &str) {
    // Another `t` doesn't parse as a watch: not ours, passed over.
    if serde_json::from_str::<ForgeWatch>(text).is_err() {
        return;
    }
    let none = serde_json::to_string(&ForgeWatching::new(vec![])).unwrap_or_default();
    app.relay.text(daemon, &none);
}

/// This hub has an App (`GITHUB_APP_ID`) and the platform says it has
/// none: its secrets weren't set (M161, docs/control-ops.md).
fn platform_has_no_app() {
    tracing::error!(
        "GITHUB_APP_ID is set here, and the platform has no GitHub App: set the App's secrets on the platform \
         (GITHUB_APP_ID, GITHUB_APP_SLUG, GITHUB_APP_WEBHOOK_SECRET, GITHUB_APP_PRIVATE_KEY)"
    );
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
}
