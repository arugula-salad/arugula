//! Who drives an account's Cloud workspaces, and Cloud's flags.
//!
//! Cloud workspaces are hosted workspaces now ([`crate::hosted`], M179):
//! `arugulad` in a rented sandbox, attached to a project. The Managoat-backed
//! ones (aru-046, M137) were removed in M189 and M190, with their tables,
//! routes and worker. What's left here is what hosted workspaces still use:
//! [`CloudDriver`], the `cloud.drive` grant's check on a machine that drives
//! its owner's workspaces, and the master key and beta allowlist flags,
//! which kept their names so a deploy's secrets don't change.

use anyhow::Result;
use arugula_control_common::{ApiError, Principal, err};
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use std::sync::Arc;

use crate::{
    Platform,
    auth::{Caller, Person},
    secrets::SecretCipher,
};

/// Cloud's flags and their env vars, with the names they had on control
/// (M137): control takes them (`#[command(flatten)]`), and so does the
/// platform's own binary (M140).
#[derive(clap::Args, Debug, Clone)]
#[group(id = "cloud")]
pub struct Args {
    /// Separate 32-byte hex master key for secrets: it wraps each project's data key. Back up with deployment secrets.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY", hide_env_values = true)]
    pub cloud_secret_key: Option<String>,
    /// The master key before this one, only while it's rotated
    /// (docs/control-ops.md): it still opens what it wrapped.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY_PREVIOUS", hide_env_values = true)]
    pub cloud_secret_key_previous: Option<String>,
    /// The master key after this one, only while it's rotated, before it
    /// seals anything: it opens what a machine that has it as
    /// ARUGULA_CLOUD_SECRET_KEY wraps.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY_NEXT", hide_env_values = true)]
    pub cloud_secret_key_next: Option<String>,
    /// Limited beta allowlist: hosted workspaces these accounts make run on
    /// the platform's inference key (M185, `hosted::inference`). Empty: none
    /// do.
    #[arg(long, env = "ARUGULA_CLOUD_ACCOUNTS", value_delimiter = ',')]
    pub cloud_accounts: Vec<String>,
}

impl Args {
    /// The master key these flags give (M209: it wraps each project's data
    /// key, [`crate::keys`]), with the previous or next one while it's
    /// rotated. Unset is none; set but empty is refused, as any key that
    /// isn't one is.
    pub fn master_key(&self) -> Result<Option<SecretCipher>> {
        self.cloud_secret_key
            .as_deref()
            .map(|k| {
                anyhow::ensure!(
                    !k.trim().is_empty(),
                    "ARUGULA_CLOUD_SECRET_KEY is set but empty: unset it, or give a key"
                );
                SecretCipher::new(k)?
                    .with_previous(self.cloud_secret_key_previous.as_deref())?
                    .with_next(self.cloud_secret_key_next.as_deref())
            })
            .transpose()
    }
}

/// Someone driving the account's Cloud workspaces: a signed-in person, a
/// CLI device, or (#1094) one of the account's daemons, for the agents on
/// that machine. A daemon acts as its account (its owner), so anyone who
/// may run things on that machine drives the owner's Cloud workspaces
/// through it; that was decided, not overlooked. Since M130 (#1126) the
/// daemon needs a live `cloud.drive` grant over its account ("Grants, not
/// impersonation"): every joined machine has one until its owner revokes
/// it, kept by the platform ([`crate::grants::can_drive_cloud`]). Who's asking is
/// the hub's to say (the gate), with a person's same-origin check. Routes
/// an agent may call take one (hosted workspaces, linked repos' items and
/// drafts); the rest stay a person's ([`Person`]).
pub struct CloudDriver(pub Person);

/// What a machine without a live `cloud.drive` grant is told (the hub's
/// relay says it too, so it's `arugula-platform-api`'s).
pub use arugula_platform_api::no_cloud_drive;

impl FromRequestParts<Arc<Platform>> for CloudDriver {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, ApiError> {
        let Caller(who) = Caller::from_request_parts(parts, app).await?;
        let (account, daemon) = match who {
            Principal::Person { account } | Principal::Cli { account, .. } => {
                return Ok(CloudDriver(Person { account }));
            }
            Principal::Daemon { account, daemon } => (account, daemon),
        };
        if !crate::grants::can_drive_cloud(app, &daemon, &account).await? {
            let name = app
                .cache
                .daemons(&*app.hub, std::slice::from_ref(&daemon))
                .await?
                .remove(&daemon)
                .map_or(daemon.clone(), |d| d.name);
            tracing::warn!(what = "cloud_drive", %account, %daemon, why = "no live cloud.drive grant", "refused");
            return Err(err(StatusCode::FORBIDDEN, &no_cloud_drive(&name)));
        }
        Ok(CloudDriver(Person { account }))
    }
}

/// A clone URL a hosted workspace may fetch: HTTPS, with no credentials in
/// it.
pub fn https_repository(repository: &str) -> bool {
    url::Url::parse(repository).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
