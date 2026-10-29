//! The hub's side of the seam with the platform (#314,
//! `arugula-platform-api`): what Arugula control's hub (`arugula-control-base`)
//! needs of its platform, and nothing that changes with the platform's
//! features. The hub depends on this crate, never on the platform's base
//! (`arugula-platform-base`), so an edit there doesn't rebuild the hub's
//! library.
//!
//! - [`HubPort`] (everything the platform asks the hub) and [`PlatformPort`]
//!   (everything the hub asks the platform), their types being
//!   `arugula_control_wire::internal`'s;
//! - [`remote`], both ports over the private network, and [`auth`], who's
//!   asking as the hub found out;
//! - [`page`], the platform page the hub serves on the platform's host;
//! - [`in_process`], the platform in the hub's process, which the
//!   `arugula-control` binary makes and hands the hub;
//! - [`Config`], what the platform is told when it starts, and a few
//!   names and checks both sides use.

pub mod auth;
pub mod hub;
pub mod in_process;
pub mod page;
pub mod planter_backup;
pub mod port;
pub mod remote;

pub use hub::HubPort;
pub use port::{Archived, Deleted, PlatformPort, PublicPreview, TurnAnswer};

/// What the platform is told when it starts.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The URL people reach control at (the hub's first site), without a
    /// trailing slash.
    pub public_url: String,
    /// Every URL control answers at, `public_url` first (#507), without a
    /// trailing slash.
    pub sites: Vec<String>,
    /// The platform page's origin (`https://app.arugula.io`, M151), where
    /// its issues, threads and sessions are: empty when it has none, and
    /// links to the page are on `public_url` then ([`Config::page`]).
    pub page_url: String,
    /// Free relay traffic per account per month, with billing on.
    pub relay_free_bytes: u64,
}

impl Config {
    /// Where a link to the platform page starts (`https://app.arugula.io`
    /// for `/issues/iss_…`), without a trailing slash: its own origin, else
    /// control's public URL.
    pub fn page(&self) -> &str {
        let page = self.page_url.trim_end_matches('/');
        if page.is_empty() { self.public_url.trim_end_matches('/') } else { page }
    }

    /// The site a request came in on, by its Host header; the first when
    /// it's none of them.
    pub fn site(&self, headers: &axum::http::HeaderMap) -> &str {
        let host = headers.get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).map(str::to_ascii_lowercase);
        let authority = |s: &str| {
            let u = url::Url::parse(s).ok()?;
            let host = u.host_str()?.to_ascii_lowercase();
            Some(u.port().map_or(host.clone(), |p| format!("{host}:{p}")))
        };
        host.and_then(|h| self.sites.iter().find(|s| authority(s).as_deref() == Some(h.as_str())))
            .map_or(&self.public_url, |s| s)
    }
}

/// The hub's change-feed sequence a call after a write waits for
/// (`cache::fresh` on the platform's side), as the gateway sends it.
pub const SEQ_HEADER: &str = "x-arugula-hub-seq";

/// The Server-Timing header, which the platform's calls to the hub fill and
/// the gateway adds to.
pub const TIMING_HEADER: &str = "server-timing";

/// What the platform says when it has no GitHub App: the hub, which knows
/// whether control should have one, logs it as a misconfiguration.
pub const NO_APP: &str = "this control has no GitHub App";

/// The message a machine whose `cloud.drive` grant is gone gets.
pub fn no_cloud_drive(name: &str) -> String {
    format!(
        "{name} can't drive this account's Cloud workspaces: its cloud.drive grant was revoked. \
         To restore it, run `arugula grants restore {name}` on a device logged in to the account (`arugula login`)"
    )
}

/// The public previews' domain (`ARUGULA_PREVIEW_DOMAIN`, M187): a domain
/// name, lowercased, or none.
pub fn preview_domain(raw: &str) -> anyhow::Result<Option<String>> {
    let d = raw.trim().trim_matches('.').to_ascii_lowercase();
    if d.is_empty() {
        return Ok(None);
    }
    if d.contains(['/', ':', ' ']) || !d.contains('.') {
        anyhow::bail!("ARUGULA_PREVIEW_DOMAIN {raw:?}: a domain name, like arugula.site");
    }
    Ok(Some(d))
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
