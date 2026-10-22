//! Arugula control (`arugula-control`): accounts, devices, the
//! directory and the relay, for people who don't run a tailnet and for
//! teams. Anyone can run it; the hosted one runs this code.
//!
//! It holds metadata only. Terminal bytes travel end to end between a
//! client device and a daemon (docs/control-e2e.md): control introduces
//! them and relays opaque messages, and the keys it distributes are signed
//! by the account's own devices, so it can refuse service but can't read.
//! The exception is chat (M122): a team's channels and DMs and the threads
//! about panes are held in plain text, with the lines people choose to
//! quote from a pane; the rest of a pane never is. Chat is the platform's
//! (`arugula_platform::chat`, M138), behind this hub's gate.

mod account;
mod activity;
#[cfg(test)]
mod activity_wire;
mod analytics;
mod api;
mod app_login;
mod app_signin;
#[cfg(test)]
mod app_signin_wire;
mod auth;
mod blocks;
#[cfg(test)]
mod blocks_wire;
mod census;
#[cfg(test)]
mod chat_bench;
#[cfg(test)]
mod chat_wire;
mod db;
mod doc_links;
#[cfg(test)]
mod doc_links_wire;
mod forge;
mod forge_send;
#[cfg(test)]
mod forge_wire;
mod gateway;
#[cfg(test)]
mod gateway_wire;
#[cfg(test)]
mod grants_wire;
mod guest_jump;
mod hosted;
mod hosted_agents;
#[cfg(test)]
mod hosted_wire;
mod hosted_work;
#[cfg(test)]
mod hub_latency_bench;
mod hub_port;
mod internal;
mod labs_flags;
mod limit;
mod maintenance;
mod notice;
mod passkey;
mod plans;
mod platform_host;
#[cfg(test)]
mod platform_host_wire;
mod previews;
#[cfg(test)]
mod previews_wire;
mod push;
#[cfg(test)]
mod push_notices;
mod relay;
mod roots;
#[cfg(test)]
mod routes;
#[cfg(test)]
mod routing_wire;
mod teams;
mod telemetry;
#[cfg(test)]
mod two_sites;
mod wake;
#[cfg(test)]
mod wake_wire;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context as _;
use arugula_control_wire as wire;
use axum::{
    Json, Router,
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
    serve::ListenerExt,
};
use clap::Parser;
use rust_embed::Embed;
use tracing::info;

#[derive(Parser, Debug)]
#[command(version, about = "Arugula control: accounts, devices, the directory and the relay")]
struct Args {
    /// Address to listen on (put TLS in front: Caddy, Fly, `tailscale serve`).
    /// Port 0 picks a free one, recorded in `listen` beside the database.
    #[arg(long, default_value = "127.0.0.1:7690", env = "ARUGULA_CONTROL_LISTEN")]
    listen: SocketAddr,

    /// The URL people and daemons reach this at, without a trailing slash
    /// (`https://control.example.com`). Port 0 is the port --listen got.
    #[arg(long, default_value = "http://127.0.0.1:7690", env = "ARUGULA_CONTROL_URL")]
    public_url: String,

    /// More URLs this answers at, comma-separated (#507: the old one while
    /// control moves). A browser is served as whichever it came in on, with
    /// its own origin, passkeys and cookies; daemons may use any of them.
    #[arg(long, value_delimiter = ',', env = "ARUGULA_CONTROL_ALSO_URLS")]
    also_url: Vec<String>,

    /// Send browsers that come in on an --also-url to the same page at
    /// --public-url, once everyone has moved (#507). Daemons, the API and
    /// sign-ins already under way are still answered there.
    #[arg(long, env = "ARUGULA_CONTROL_ALSO_REDIRECT")]
    also_redirect: bool,

    /// Serve blocks on origins of their own under this domain (M50:
    /// `b-<key>.<domain>`, `arugula.run` for the hosted one): a registrable
    /// domain used for nothing else, with a wildcard DNS name and
    /// certificate pointed here. Off when unset (docs/control-ops.md).
    #[arg(long, env = "ARUGULA_CONTROL_BLOCK_DOMAIN")]
    block_domain: Option<String>,

    /// Serve the platform page on this host (M151: `app.arugula.io`; dev
    /// `app.localhost`), a bare host name whose DNS points here: the hub
    /// answers it with the platform's page and routes, and nothing of its
    /// own. Never one of control's own sites, nor under the block domain.
    /// Off when unset (docs/control-ops.md, "The platform page's host").
    #[arg(long, env = "ARUGULA_PLATFORM_HOST")]
    platform_host: Option<String>,

    /// Serve the platform page from this directory instead of the built-in
    /// copy (development), with the platform in process. Out of process the
    /// platform serves its own (its `--static-dir`).
    #[arg(long)]
    platform_static_dir: Option<PathBuf>,

    /// The SQLite database.
    #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,

    /// GitHub App (or OAuth app) client id, for signing in with GitHub.
    #[arg(long, env = "GITHUB_CLIENT_ID")]
    github_client_id: Option<String>,

    #[arg(long, env = "GITHUB_CLIENT_SECRET", hide_env_values = true)]
    github_client_secret: Option<String>,

    /// GitHub's web and API bases (tests point these at a fake).
    #[arg(long, default_value = "https://github.com", hide = true)]
    github_url: String,
    #[arg(long, default_value = "https://api.github.com", hide = true)]
    github_api: String,

    /// The GitHub App (M40): its flags are the platform's (M161), with the
    /// names they always had. In process the hub hands the App to the
    /// platform; out of process it needs only the id and slug, and the
    /// platform holds the key and webhook secret (docs/control-ops.md).
    #[command(flatten)]
    github_app: arugula_platform::forge::Args,
    /// The App's client id and secret sign people in when
    /// GITHUB_CLIENT_ID/SECRET aren't set: the hub's ("The hub stays the
    /// identity provider").
    #[arg(long, env = "GITHUB_APP_CLIENT_ID")]
    github_app_client_id: Option<String>,
    #[arg(long, env = "GITHUB_APP_CLIENT_SECRET", hide_env_values = true)]
    github_app_client_secret: Option<String>,
    /// How often subscribed daemons hear the webhook path is up (tests).
    #[arg(long, default_value_t = 60, hide = true)]
    forge_heartbeat_secs: u64,

    /// Cloud workspaces: their flags and env vars are the platform's
    /// (M137), with the names they always had.
    #[command(flatten)]
    cloud: arugula_platform::cloud::Args,

    /// Hosted workspaces' boxes (M176): the platform's flags, used when
    /// it runs in process.
    #[command(flatten)]
    compute: arugula_platform::compute::Args,

    /// Hosted workspaces (M179): the platform's flags, used when it runs
    /// in process.
    #[command(flatten)]
    hosted: arugula_platform::hosted::Args,

    /// The platform's own database (M143): `postgres://` puts it on
    /// Postgres. Without it, a second connection to `--db`, as always.
    #[command(flatten)]
    platform_db: arugula_platform::db::Args,

    /// Billing (M22): its flags and env vars are the platform's (M140),
    /// with the names they always had. The hub only needs to know it's on.
    #[command(flatten)]
    billing: arugula_platform::billing::Args,

    /// Where the platform runs (#1150): `in-process`, inside this hub (the
    /// default, and the rollback), or the URL of an `arugula-platform` on
    /// the private network (`http://arugula-platform.internal:7700`, or
    /// through Flycast to its machines, `http://arugula-platform.flycast:7700`,
    /// M144), which this hub then forwards every platform route to as its
    /// gateway.
    #[arg(long, default_value = "in-process", env = "ARUGULA_PLATFORM")]
    platform: String,

    /// The key the hub and an out-of-process platform share (hex, at least
    /// 32 bytes): principals and the calls between them are signed with it.
    #[arg(long, env = "ARUGULA_PLATFORM_KEY", hide_env_values = true)]
    platform_key: Option<String>,

    /// Where the hub answers the platform's calls (`HubPort` over HTTP), on
    /// the private network only: never a public port. Defaults to
    /// `[::]:7691` with an out-of-process platform, and is off otherwise.
    #[arg(long, env = "ARUGULA_CONTROL_INTERNAL_LISTEN")]
    internal_listen: Option<SocketAddr>,
    /// Free relay traffic per account per month, in MB (with billing on).
    #[arg(long, default_value_t = 10_000, env = "ARUGULA_RELAY_FREE_MB")]
    relay_free_mb: u64,

    /// Relay limits per account (#174), 0 for none: client sockets at
    /// once, machines dialed in at once, and (while billing is off) MB a
    /// day before its relayed traffic slows down.
    #[arg(long, default_value_t = 32, env = "ARUGULA_RELAY_MAX_SOCKETS")]
    relay_max_sockets: usize,
    #[arg(long, default_value_t = 50, env = "ARUGULA_RELAY_MAX_MACHINES")]
    relay_max_machines: usize,
    #[arg(long, default_value_t = 2_000, env = "ARUGULA_RELAY_DAILY_MB")]
    relay_daily_mb: u64,
    /// Relay sockets at once, every account's together (#344), 0 for no
    /// limit. Keep it below the proxy's connection limit (Fly's
    /// `soft_limit`), so a full relay still leaves room for pages and
    /// sign-ins.
    #[arg(long, default_value_t = 5_000, env = "ARUGULA_RELAY_MAX_TOTAL")]
    relay_max_total: usize,

    /// Product analytics (analytics.rs): a PostHog project's API key turns
    /// them on (POSTHOG_API_KEY), sent to its host. Off when unset.
    #[arg(long, env = "POSTHOG_API_KEY", hide_env_values = true)]
    posthog_api_key: Option<String>,
    #[arg(long, default_value = "https://us.i.posthog.com", env = "POSTHOG_HOST")]
    posthog_host: String,

    /// Off-site backup with Litestream (#174).
    #[command(flatten)]
    backup: arugula_control_common::backup::Litestream,

    /// Push endpoints allowed besides the browsers' push services, as
    /// host:port (tests).
    #[arg(long = "push-host", hide = true)]
    push_hosts: Vec<String>,

    /// Take shared Docs whatever the disk looks like (tests: a CI pod's
    /// disk is its node's, which other pods fill).
    #[arg(long, hide = true)]
    no_disk_guard: bool,

    /// Behind a proxy that puts the client's IP in a header (Fly:
    /// `Fly-Client-IP`), use it for rate limits. Only set this when every
    /// request comes through that proxy.
    #[arg(long, env = "ARUGULA_CONTROL_PROXY_HEADER")]
    trust_proxy_header: Option<String>,

    /// Serve the web client from this directory instead of the built-in
    /// copy (development).
    #[arg(long)]
    static_dir: Option<PathBuf>,

    /// An ssh jump host for guests of daemons behind NAT (M65: a daemon's
    /// `arugula share --guest` goes through it when the daemon has no
    /// address of its own), listening here. Off unless set.
    #[arg(long, env = "ARUGULA_CONTROL_GUEST_SSH")]
    guest_ssh: Option<SocketAddr>,

    /// The host[:port] guests dial for the jump host [default: the public
    /// URL's host, and --guest-ssh's port].
    #[arg(long, env = "ARUGULA_CONTROL_GUEST_SSH_HOST")]
    guest_ssh_host: Option<String>,
}

pub struct Github {
    pub client_id: String,
    pub client_secret: String,
    pub url: String,
    pub api: String,
}

pub struct Config {
    pub push_hosts: Vec<String>,
    pub relay_free_bytes: u64,
    /// Billing is on (there's a Stripe key): the platform bills, and the
    /// relay's free allowance replaces its daily cap (`plans`).
    pub billing: bool,
    pub public_url: String,
    /// `public_url`'s origin, as browsers send it.
    pub origin: String,
    /// Every URL this answers at, `public_url`'s first (#507).
    pub sites: Vec<Site>,
    /// Browsers on any but the first go to it (`--also-redirect`).
    pub also_redirect: bool,
    /// Blocks' origins are `b-<key>.<this>` (M50, `blocks.rs`).
    pub block_domain: Option<String>,
    /// Public previews are `<id>.<this>` (M187, `previews.rs`).
    pub preview_domain: Option<String>,
    /// The platform page's host (M151, `platform_host.rs`), and where its
    /// files come from when the platform is in process.
    pub platform_host: Option<platform_host::PlatformHost>,
    pub platform_page: arugula_platform::page::Page,
    pub github: Option<Github>,
    /// The GitHub App's own OAuth client (`GITHUB_APP_CLIENT_ID`/`SECRET`):
    /// a person's send runs its user authorization (M168), whichever
    /// client signs people in.
    pub github_app_client: Option<Github>,
    pub static_dir: Option<PathBuf>,
}

/// One URL control answers at (#507).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// Without a trailing slash: `https://control.example.com`.
    pub url: String,
    /// As browsers send it.
    pub origin: String,
    /// `host[:port]`, as the Host header says it.
    pub authority: String,
}

impl Site {
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let url = url.trim_end_matches('/').to_owned();
        let u = url::Url::parse(&url)?;
        let host = u.host_str().context("a URL with a host")?;
        let authority = match u.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_owned(),
        };
        Ok(Site { origin: u.origin().ascii_serialization(), authority: authority.to_ascii_lowercase(), url })
    }

    /// Its host name, which passkeys are made for (WebAuthn's RP ID).
    pub fn host(&self) -> &str {
        self.authority.rsplit_once(':').filter(|(_, p)| p.parse::<u16>().is_ok()).map_or(&self.authority, |(h, _)| h)
    }
}

impl Config {
    /// The site a request came in on, by its Host header; the first when
    /// it's none of them.
    pub fn site(&self, headers: &HeaderMap) -> &Site {
        let host = headers.get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).map(str::to_ascii_lowercase);
        host.and_then(|h| self.sites.iter().find(|s| s.authority == h)).unwrap_or(&self.sites[0])
    }
}

pub struct App {
    pub cfg: Config,
    pub db: db::Db,
    pub http: reqwest::Client,
    pub relay: relay::Relay,
    pub passkeys: passkey::Challenges,
    /// The platform page's sign-in codes not yet swapped (M152).
    pub app_codes: app_signin::Codes,
    /// GitHub authorizations under way for a person's send (M168).
    pub sends: forge_send::Starts,
    pub limits: limit::Limits,
    pub vapid: push::Vapid,
    /// Control's hosted key (M178): it signs the hosted machines enrolled
    /// under a project's delegation, and nothing else.
    pub hosted_key: arugula_e2e::hosted::HostedKey,
    /// The GitHub App's slug (M40), when control has one; the App itself
    /// is the platform's (M161). And daemons' subscriptions to its pokes.
    pub github_app: Option<String>,
    pub forge: forge::Watches,
    /// The desktop app's sign-ins in progress (M48).
    pub app_logins: app_login::Tickets,
    /// Daemon signatures (and join proofs) already taken.
    pub daemon_sigs: auth::Replays,
    /// Who still signs requests at an old URL (#535).
    pub old_hosts: auth::OldHosts,
    /// The ssh jump host for guests (M65), when it's on, and the routes
    /// daemons registered for it.
    pub jump: Option<guest_jump::Jump>,
    pub guest_routes: guest_jump::Routes,
    /// Blocks' relay sockets open (M50).
    pub blocks: blocks::Open,
    /// Public previews: the platform's recent answers, and requests open
    /// (M187).
    pub previews: previews::Previews,
    /// Product analytics, when they're on.
    pub analytics: analytics::Analytics,
    /// Shared Docs: the disk guard and the open counts (#1064).
    pub docs: doc_links::Docs,
    /// The platform (#1153): chat, Cloud, billing, projects and grants
    /// moved into it (M137-M139, step 2), and it reaches the hub only
    /// through `hub_port`. In this process, or its own behind this hub's
    /// gateway (M140).
    pub platform: gateway::PlatformAt,
    /// The maintenance switch (M141): while it's on, every platform route
    /// answers 503 before anything is forwarded.
    pub maintenance: maintenance::Switch,
    /// The platform's view of the hub, attached to this app by
    /// [`App::start`].
    pub hub_port: Arc<hub_port::InProcess>,
}

impl App {
    /// The app in its `Arc`, with the platform's `HubPort` attached to it.
    pub fn start(self) -> Arc<Self> {
        let app = Arc::new(self);
        app.hub_port.attach(Arc::downgrade(&app));
        app
    }
}

/// The platform in process, on its own connection to `db` (memory for
/// tests), and the `HubPort` it reaches the hub through, attached by
/// [`App::start`].
#[allow(clippy::too_many_arguments)]
fn platform(
    db: arugula_platform::db::Db,
    cfg: arugula_platform::Config,
    stripe: Option<arugula_platform::billing::Stripe>,
    cloud: Option<arugula_platform::cloud::Cloud>,
    github: Option<arugula_platform::forge::GithubApp>,
    compute: Option<Arc<dyn arugula_platform::compute::Substrate>>,
    hosted: arugula_platform::hosted::Config,
    http: &reqwest::Client,
    analytics: &analytics::Analytics,
) -> (gateway::PlatformAt, Arc<hub_port::InProcess>) {
    let hub = Arc::new(hub_port::InProcess::default());
    let mut p = arugula_platform::Platform::new(cfg, db, hub.clone(), http.clone(), analytics.clone());
    p.stripe = stripe;
    p.cloud = cloud;
    p.github = github;
    p.compute = compute;
    // The master key (M209), which the hosted config was given.
    p.secret_key = hosted.cipher.clone();
    p.hosted = hosted;
    (gateway::PlatformAt::InProcess(Arc::new(p)), hub)
}

#[cfg(test)]
impl App {
    /// An app for tests: an in-memory database, nothing configured.
    pub fn for_tests(public_url: &str) -> Self {
        let db = db::Db::memory();
        let vapid = push::Vapid::load(&db).unwrap();
        let hosted_key = hosted::load_key(&db).unwrap();
        let http = reqwest::Client::new();
        let cfg = arugula_platform::Config {
            public_url: public_url.into(),
            sites: vec![public_url.into()],
            relay_free_bytes: 0,
        };
        let (platform, hub_port) = platform(
            arugula_platform::db::Db::memory(),
            cfg,
            None,
            None,
            None,
            None,
            Default::default(),
            &http,
            &analytics::Analytics::default(),
        );
        App {
            cfg: Config {
                push_hosts: vec![],
                relay_free_bytes: 0,
                billing: false,
                public_url: public_url.into(),
                origin: origin_of(public_url).unwrap(),
                sites: vec![Site::new(public_url).unwrap()],
                also_redirect: false,
                block_domain: None,
                preview_domain: None,
                platform_host: None,
                platform_page: Default::default(),
                github: None,
                github_app_client: None,
                static_dir: None,
            },
            db,
            http,
            relay: Default::default(),
            passkeys: Default::default(),
            app_codes: Default::default(),
            sends: Default::default(),
            limits: limit::Limits::new(None),
            vapid,
            hosted_key,
            github_app: None,
            forge: Default::default(),
            app_logins: Default::default(),
            daemon_sigs: Default::default(),
            old_hosts: Default::default(),
            jump: None,
            guest_routes: Default::default(),
            blocks: Default::default(),
            previews: Default::default(),
            analytics: Default::default(),
            docs: Default::default(),
            platform,
            maintenance: Default::default(),
            hub_port,
        }
    }
}

// API errors, typed answers and daily meters are shared with the
// platform (`arugula-control-common`).
pub use arugula_control_common::{ApiError, day, err, refusal, reply};

/// Every route control answers on its own site. routes.toml lists each one
/// with the process that answers it, hub or platform (#1152), and its test
/// (routes.rs) reads the paths from here.
fn routes() -> Routes {
    Routes::default()
        .route(wire::CONTROL_JSON, get(control_json))
        .route("/auth/github", get(auth::github_start))
        .route("/auth/github/callback", get(auth::github_callback))
        .route("/auth/logout", post(auth::logout))
        // The platform page's sign-in (M152): the code on control's site,
        // the swap and signing out on the platform page's.
        .route("/signin/platform", get(app_signin::start))
        .route(app_signin::APP_SESSION, post(app_signin::swap))
        .route(app_signin::APP_SIGNOUT, post(app_signin::signout))
        .route("/auth/app", post(app_login::ask))
        .route("/auth/app/{id}/redeem", post(app_login::redeem))
        .route("/api/app-login/{id}", get(app_login::show))
        .route("/api/app-login/{id}/allow", post(app_login::allow))
        .route("/auth/passkey/register", post(passkey::register_start))
        .route("/auth/passkey/register/finish", post(passkey::register_finish))
        .route("/auth/passkey/login", post(passkey::login_start))
        .route("/auth/passkey/login/finish", post(passkey::login_finish))
        .route("/api/me", get(api::me))
        .route("/api/me/name", post(api::set_name))
        .route("/api/me/sessions", get(account::sessions))
        .route("/api/me/sessions/end-all", post(account::end_all))
        .route("/api/me/sessions/device", post(account::session_device))
        .route("/api/me/sessions/{id}/end", post(account::end_session))
        .route("/api/me/passkeys", get(account::passkeys))
        .route("/api/me/passkeys/{id}/remove", post(account::remove_passkey))
        .route("/api/me/delete", get(account::preview).post(account::delete))
        .route("/api/me/notices/{id}/seen", post(teams::notice_seen))
        .route("/api/devices", get(api::devices).post(api::enroll))
        .route("/api/devices/{id}", get(api::device))
        .route("/api/devices/{id}/approve", post(api::approve))
        .route("/api/devices/{id}/pin", post(api::pin))
        .route("/api/devices/{id}/reject", post(api::reject))
        .route("/api/revocations", post(api::revoke))
        .route("/api/recovery", post(api::add_recovery))
        .route(wire::JOIN, post(api::join))
        .route(wire::JOIN_POLL, get(api::join_poll))
        .route("/api/joins/{code}", get(api::join_show))
        .route("/api/joins/{code}/approve", post(api::join_approve))
        .route("/api/joins/{code}/reject", post(api::join_reject))
        .route("/api/daemons/{id}/team", post(api::move_daemon))
        .route(wire::TRUST, get(api::daemon_trust))
        .route(wire::LEAVE, post(api::daemon_leave))
        .route(
            wire::telemetry::TELEMETRY,
            post(telemetry::report).layer(axum::extract::DefaultBodyLimit::max(telemetry::MAX_BODY)),
        )
        .route(wire::telemetry::ME_TELEMETRY, get(telemetry::page_consent).post(telemetry::set_page_consent))
        .route(
            wire::telemetry::PAGE,
            post(telemetry::page).layer(axum::extract::DefaultBodyLimit::max(telemetry::MAX_PAGE_BODY)),
        )
        .route("/api/directory", get(api::directory))
        .route("/api/shares/{daemon}", post(teams::answer_share))
        .route("/api/people", get(teams::person))
        .route("/api/flags", get(labs_flags::list).post(labs_flags::set))
        .route("/api/teams", get(teams::list).post(teams::create))
        .route("/api/teams/{id}/roster", post(teams::set_roster))
        .route("/api/teams/{id}/invites", post(teams::invite))
        .route("/api/teams/{id}/presigned", get(teams::list_presigned))
        .route("/api/teams/{id}/presigned/{key}", axum::routing::delete(teams::cancel_presigned))
        .route("/api/teams/{id}/requests/{account}/reject", post(teams::reject))
        .route("/api/teams/{id}/lock", post(teams::lock))
        .route("/api/invites/{team}/{code}", get(teams::show_invite))
        .route("/api/invites/{team}/{code}/accept", post(teams::accept_invite))
        .route("/api/invites/{team}/{code}/preview", get(teams::preview_invite))
        .route("/api/presigned/{team}/{key}", get(teams::show_presigned))
        .route("/api/presigned/{team}/{key}/preview", get(teams::preview_presigned))
        .route(wire::TEAM, get(teams::daemon_team))
        .route(wire::PEERS, get(teams::daemon_peers))
        .route(wire::TEAMS, get(teams::daemon_teams))
        .route(wire::ACCESS, post(teams::daemon_access))
        .route("/api/relay/link/{id}", get(relay::link))
        .route("/api/push/subscribe", post(push::subscribe))
        .route("/api/push/unsubscribe", post(push::unsubscribe))
        .route(wire::PUSH_SUBS, get(push::daemon_subs))
        .route(wire::PUSH, post(push::daemon_send))
        .route("/api/doc-links", get(doc_links::list))
        .route("/api/doc-links/quota", get(doc_links::quota))
        .route("/api/doc-links/{id}", axum::routing::delete(doc_links::remove))
        .route("/api/doc-links/{id}/v/{ver}/parts/{n}", axum::routing::put(doc_links::put_part))
        .route("/api/doc-links/{id}/v/{ver}/commit", post(doc_links::commit))
        // The viewer where there's no block domain (`doc_links::serve`).
        .route("/d/{*rest}", axum::routing::any(doc_links::fallback))
        // Hosted machines under a project's delegation (M178).
        .route("/api/hosted/delegations", get(hosted::delegations).post(hosted::delegate))
        .route("/api/hosted/delegations/revoke", post(hosted::revoke))
        .route(wire::hosted::ENROLL, post(hosted::enroll))
        .route(wire::hosted::HOSTED_TRUST, get(hosted::trust))
        .route(wire::hosted::HOSTED_TURNS, post(hosted::turn))
        .route(wire::hosted::HOSTED_PREVIEWS, get(hosted::previews))
        .route("/api/hosted/turns/{turn}/end", post(hosted::turn_end))
        .route(wire::work::WORK, get(hosted_work::work))
        .route(wire::work::WORK_REPORT, post(hosted_work::report))
        .route(wire::hosted::SPEC, get(hosted::spec))
        .route(wire::hosted::SETUP, post(hosted::setup))
        .route("/api/hosted/machines/{id}", axum::routing::delete(hosted::remove))
        .route(arugula_control_wire::transcripts::HOSTED_TRANSCRIPTS, post(hosted_agents::transcript))
        .route("/api/daemon/github/token", post(forge::daemon_token))
        .route(wire::RELAY_DIAL, get(relay::dial))
        .route("/api/relay/c/{id}", get(relay::client))
        .route("/api/relay/m", get(relay::many))
}

/// A router that notes its paths too, for routes.toml's test.
#[derive(Default)]
struct Routes {
    router: Router<Arc<App>>,
    #[cfg(test)]
    paths: Vec<&'static str>,
}

impl Routes {
    fn route(mut self, path: &'static str, methods: axum::routing::MethodRouter<Arc<App>>) -> Self {
        #[cfg(test)]
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}

pub fn router(app: Arc<App>) -> Router {
    // The platform's routes: in process behind the gate that says who's
    // asking, or forwarded to it (M140); either way behind the maintenance
    // switch.
    let platform = gateway::router(&app);
    routes()
        .router
        .merge(platform)
        .fallback(asset)
        .layer(axum::middleware::from_fn_with_state(app.clone(), analytics::requests))
        .layer(axum::middleware::from_fn_with_state(app.clone(), account::note_agent))
        .layer(axum::middleware::from_fn_with_state(app.clone(), moved))
        .layer(axum::middleware::from_fn_with_state(app.clone(), auth::verify_daemon))
        .layer(axum::middleware::map_response_with_state(app.clone(), headers))
        // A block's origin never reaches anything above (M50).
        .layer(axum::middleware::from_fn_with_state(app.clone(), blocks::split))
        // Outermost: the platform page's host reaches only the platform's
        // routes and its page (M151).
        .layer(axum::middleware::from_fn_with_state(app.clone(), platform_host::split))
        // Outermost of all: a public preview's host reaches nothing of
        // control's (M187).
        .layer(axum::middleware::from_fn_with_state(app.clone(), previews::split))
        .with_state(app)
}

/// With `--also-redirect` (#507): a browser's page on an old site goes to
/// the same page on the first. What daemons, CLIs and pages already open
/// call (`/api/…`, `/control.json`), sign-ins under way (`/auth/…`) and
/// GitHub's webhook are still answered where they are.
async fn moved(
    axum::extract::State(app): axum::extract::State<Arc<App>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let path = req.uri().path();
    let page = matches!(*req.method(), axum::http::Method::GET | axum::http::Method::HEAD)
        && !["/api/", "/auth/", "/github/", "/.well-known/"].iter().any(|p| path.starts_with(p))
        && path != "/control.json"
        && !req.headers().contains_key(axum::http::header::UPGRADE);
    if app.cfg.also_redirect && page && app.cfg.site(req.headers()) != &app.cfg.sites[0] {
        let pq = req.uri().path_and_query().map_or("/", |p| p.as_str());
        return axum::response::Redirect::permanent(&format!("{}{pq}", app.cfg.public_url)).into_response();
    }
    next.run(req).await
}

async fn control_json(
    axum::extract::State(app): axum::extract::State<Arc<App>>,
    headers: HeaderMap,
) -> Json<arugula_control_wire::ControlInfo> {
    let site = app.cfg.site(&headers);
    // Passkeys need a domain name: WebAuthn refuses IP addresses.
    let passkeys = url::Url::parse(&site.url).is_ok_and(|u| matches!(u.host(), Some(url::Host::Domain(_))));
    Json(arugula_control_wire::ControlInfo {
        control: true,
        // The one asked: a page's links and relay are on its own site.
        url: site.url.clone(),
        // Where control is now, and everywhere it answers (#507): a daemon
        // that can move to `primary` does, and lets every site frame its
        // blocks.
        primary: app.cfg.public_url.clone(),
        urls: app.cfg.sites.iter().map(|s| s.url.clone()).collect(),
        github: app.cfg.github.is_some(),
        passkeys,
        vapid: app.vapid.public(),
        github_app: app.github_app.clone(),
        // How daemons sign their requests here (auth.rs): 2 takes body
        // hashes and nonces.
        daemon_auth: 2,
        // The CLI joins with a code and signs its requests (M49).
        cli_join: 1,
        // A join's approval carries the approving device's MAC under the
        // secret in the machine's link (#760), passed on to the machine.
        join_secret: 1,
        // An approval of a waiting device carries the approving device's
        // MAC under the code it shows (#761), passed on to it.
        device_secret: 1,
        // The ssh jump host for guests of daemons behind NAT (M65).
        guest_ssh: app.jump.as_ref().map(guest_jump::Jump::describe),
        // Where blocks get origins of their own (M50).
        block_domain: app.cfg.block_domain.clone(),
        // Shared Docs (#1064), and where they are opened.
        doc_links: Some(1),
        doc_viewer: Some(doc_links::viewer_url(&app.cfg.public_url, app.cfg.block_domain.as_deref())),
        // Where the platform page is (M151), when it has a host.
        platform: app.cfg.platform_host.as_ref().map(|p| p.origin.clone()),
        // What a project delegation names (M178).
        hosted_key: Some(app.hosted_key.public()),
    })
}

/// Nothing frames control's pages, and nothing on them comes from elsewhere
/// except the daemons the page connects to (any WebSocket) and the blocks it
/// shows: a block is a frame on its own site (a daemon's block domain, or a
/// web page a browser block opened), so frames may come from any https site,
/// and from control's own block origins (M50) on whatever scheme control's
/// page is on (http only for a local control).
async fn headers(axum::extract::State(app): axum::extract::State<Arc<App>>, mut res: Response) -> Response {
    let csp = |frames: &str| {
        format!(
            "default-src 'self'; connect-src 'self' wss: ws: https:; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; frame-src 'self' https:{frames}; frame-ancestors 'none'"
        )
    };
    let csp = match &app.cfg.block_domain {
        Some(d) => HeaderValue::from_str(&csp(&format!(" *.{d}:*")))
            .unwrap_or(HeaderValue::from_static("frame-ancestors 'none'")),
        None => HeaderValue::from_str(&csp("")).expect("a fixed policy"),
    };
    let h = res.headers_mut();
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    // Browsers ignore it over plain HTTP (a local control), so always.
    h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=31536000"));
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(csp);
    res
}

#[derive(Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

/// A file of the web client's build (`web/dist`), and its type.
pub fn static_file(app: &App, path: &str) -> Option<(Vec<u8>, String)> {
    match &app.cfg.static_dir {
        Some(dir) => std::fs::read(dir.join(path)).ok().map(|d| (d, mime(path).to_owned())),
        None => Assets::get(path).map(|f| (f.data.into_owned(), f.metadata.mimetype().to_owned())),
    }
}

async fn asset(axum::extract::State(app): axum::extract::State<Arc<App>>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    // Blocks' files are served on block origins only (blocks.rs).
    if path.split('/').any(|s| s == ".." || s.starts_with('.')) || path.starts_with("block/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match static_file(&app, path) {
        Some((data, mime)) => {
            let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, cache)
                .body(Body::from(data))
                .unwrap()
        }
        None if path == "index.html" => (StatusCode::NOT_FOUND, "web client not built: run `just web`").into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json" | "webmanifest") => "application/json",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub fn origin_of(url: &str) -> anyhow::Result<String> {
    let u = url::Url::parse(url)?;
    Ok(u.origin().ascii_serialization())
}

fn main() -> anyhow::Result<()> {
    // `arugula-control census`: an operator's read-only look at the
    // database (census.rs), with no server. `maintenance`: the switch
    // (maintenance.rs), set in the database the running hub reads.
    // `notice`: a notice for every account (notice.rs). `hosted-enrollment`:
    // what the platform asks as it makes a hosted box (hosted.rs). `platform`: a
    // window's tools, the platform's own code (`arugula_platform::window`).
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().is_some_and(|a| a == "census") {
        return census::run(argv.skip(1));
    }
    if argv.peek().is_some_and(|a| a == "maintenance") {
        return maintenance::run(argv.skip(1));
    }
    if argv.peek().is_some_and(|a| a == "hosted-enrollment") {
        return hosted::run(argv.skip(1));
    }
    if argv.peek().is_some_and(|a| a == "notice") {
        return notice::run(argv.skip(1));
    }
    if argv.peek().is_some_and(|a| a == "platform") {
        return arugula_platform::window::run("arugula-control platform", argv.skip(1));
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "arugula_control=info".into()),
        )
        .init();
    let a = Args::parse();
    raise_open_files();
    // Bound first, so port 0 is known before the URL is (#67).
    let l = tokio::net::TcpListener::bind(a.listen).await?;
    let listen = l.local_addr()?;
    let mut public_url = a.public_url.trim_end_matches('/').to_owned();
    if a.listen.port() == 0 {
        let at =
            a.db.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new(".")).join("listen");
        // Whole or not at all: a reader never sees half a port.
        let tmp = at.with_extension("tmp");
        std::fs::write(&tmp, listen.to_string())
            .and_then(|_| std::fs::rename(&tmp, &at))
            .with_context(|| format!("recording the listen address in {}", at.display()))?;
    }
    // Port 0 in a URL is the port --listen got, in --also-url's too.
    let port_of = |url: &str| match url::Url::parse(url) {
        Ok(mut u) if u.port() == Some(0) => {
            let _ = u.set_port(Some(listen.port()));
            u.as_str().trim_end_matches('/').to_owned()
        }
        _ => url.trim_end_matches('/').to_owned(),
    };
    public_url = port_of(&public_url);
    let also_urls: Vec<String> = a.also_url.iter().filter(|u| !u.is_empty()).map(|u| port_of(u)).collect();
    let set = |v: Option<String>| v.filter(|s| !s.is_empty());
    let github_app = a.github_app.id().map(|_| a.github_app.github_app_slug.clone());
    // Sign-in: an OAuth app's (or another App's) credentials, else the
    // GitHub App's own.
    let (client_id, client_secret) = match (set(a.github_client_id.clone()), set(a.github_client_secret.clone())) {
        (Some(i), Some(s)) => (Some(i), Some(s)),
        _ => (set(a.github_app_client_id.clone()), set(a.github_app_client_secret.clone())),
    };
    let github_app_client = match (set(a.github_app_client_id.clone()), set(a.github_app_client_secret.clone())) {
        (Some(client_id), Some(client_secret)) => {
            Some(Github { client_id, client_secret, url: a.github_url.clone(), api: a.github_api.clone() })
        }
        _ => None,
    };
    let github = match (client_id, client_secret) {
        (Some(client_id), Some(client_secret)) => {
            Some(Github { client_id, client_secret, url: a.github_url.clone(), api: a.github_api.clone() })
        }
        _ => None,
    };
    if github.is_none() {
        tracing::warn!("no GITHUB_CLIENT_ID/GITHUB_CLIENT_SECRET: GitHub sign-in is off");
    }
    let backup = arugula_control_common::backup::Backup::new(&a.backup, &a.db, "control")?;
    if let Some(b) = &backup {
        b.restore_if_missing().await?;
    }
    let db = db::Db::open(&a.db)?;
    let db_dir = a.db.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new(".")).to_owned();
    let http = roots::client(std::time::Duration::from_secs(15))?;
    let analytics = match set(a.posthog_api_key) {
        Some(key) => analytics::Analytics::start(key, &a.posthog_host, http.clone()),
        None => {
            info!("no POSTHOG_API_KEY: product analytics are off");
            analytics::Analytics::default()
        }
    };
    if let Some(b) = &backup {
        b.replicate();
    }
    let vapid = push::Vapid::load(&db)?;
    let hosted_key = hosted::load_key(&db)?;
    let stripe = a.billing.stripe()?;
    // The guests' jump host: bound now, so its port is known.
    let guest_ssh = match a.guest_ssh {
        Some(addr) => {
            let l = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("listening for guest ssh on {addr}"))?;
            let port = l.local_addr()?.port();
            // `host:port`, or a host (an IPv6 address has more than one colon).
            let (host, port) = match set(a.guest_ssh_host.clone()) {
                Some(h) => match h.split_once(':') {
                    Some((name, p)) if !p.contains(':') => {
                        (name.to_owned(), p.parse().context("--guest-ssh-host's port")?)
                    }
                    _ => (h, port),
                },
                None => (url::Url::parse(&public_url)?.host_str().unwrap_or("localhost").to_owned(), port),
            };
            Some((l, guest_jump::Jump::new(&guest_jump::key_path(&a.db), host, port)?))
        }
        None => None,
    };
    let (guest_listener, jump) = match guest_ssh {
        Some((l, j)) => (Some(l), Some(j)),
        None => (None, None),
    };
    let sites: Vec<Site> = std::iter::once(public_url.as_str())
        .chain(also_urls.iter().map(String::as_str))
        .map(Site::new)
        .collect::<anyhow::Result<_>>()?;
    let block_domain = blocks::domain(a.block_domain.as_deref().unwrap_or(""), &sites)?;
    match &block_domain {
        Some(d) => info!(domain = %d, "blocks served at b-<key>.{d}"),
        None => info!("no ARUGULA_CONTROL_BLOCK_DOMAIN: blocks aren't served through control"),
    }
    let platform_host =
        platform_host::parse(a.platform_host.as_deref().unwrap_or(""), &public_url, &sites, block_domain.as_deref())?;
    match &platform_host {
        Some(p) => info!(host = %p.host, origin = %p.origin, "the platform page is served at its host"),
        None => info!("no ARUGULA_PLATFORM_HOST: the platform page isn't served"),
    }
    let preview_domain = previews::domain(
        a.hosted.preview_domain.as_deref().unwrap_or(""),
        &sites,
        block_domain.as_deref(),
        platform_host.as_ref().map(|p| p.host.as_str()),
    )?;
    match &preview_domain {
        Some(d) => info!(domain = %d, "public previews served at <id>.{d}"),
        None => info!("no ARUGULA_PREVIEW_DOMAIN: public previews are off"),
    }
    let billing = stripe.is_some();
    let platform_key = set(a.platform_key.clone())
        .map(|k| arugula_control_common::principal::PrincipalKey::from_hex(&k))
        .transpose()
        .context("ARUGULA_PLATFORM_KEY")?;
    let (platform, hub_port) = if a.platform == "in-process" {
        // A second connection to the same file: the platform never reads
        // the hub's tables (#1153).
        let hosted = a.hosted.clone().config(&a.cloud)?;
        let cloud = a.cloud.cloud()?;
        let cfg = arugula_platform::Config {
            public_url: public_url.clone(),
            sites: sites.iter().map(|s| s.url.clone()).collect(),
            relay_free_bytes: a.relay_free_mb * 1_000_000,
        };
        info!("the platform runs in process");
        let github = a.github_app.app(&a.github_api)?;
        let compute = a.compute.clone().substrate()?;
        platform(a.platform_db.open(&a.db)?, cfg, stripe, cloud, github, compute, hosted, &http, &analytics)
    } else {
        // The platform is its own process: this hub is its gateway (M140).
        let key = platform_key.clone().context("--platform is a URL, so ARUGULA_PLATFORM_KEY is needed")?;
        info!(at = %a.platform, "the platform runs out of process, behind this hub's gateway");
        if a.github_app.has_secrets() {
            info!(
                "GITHUB_APP_PRIVATE_KEY and GITHUB_APP_WEBHOOK_SECRET are the platform's (M161): unused here, \
                 kept only for a rollback"
            );
        }
        let remote = gateway::Remote::new(&a.platform, key)?;
        (gateway::PlatformAt::Remote(remote), Arc::new(hub_port::InProcess::default()))
    };
    if a.platform_static_dir.is_some() && platform.local().is_none() {
        tracing::warn!("--platform-static-dir is for a platform in process: out of process it serves its own page");
    }
    // The hub's internal API, for an out-of-process platform.
    let internal_listen = match (a.internal_listen, &platform) {
        (Some(at), _) => Some(at),
        (None, gateway::PlatformAt::Remote(_)) => Some("[::]:7691".parse()?),
        (None, gateway::PlatformAt::InProcess(_)) => None,
    };
    let internal_listener = match internal_listen {
        Some(at) => {
            let key = platform_key.clone().context("the internal listener needs ARUGULA_PLATFORM_KEY")?;
            let l = tokio::net::TcpListener::bind(at)
                .await
                .with_context(|| format!("listening for the platform on {at}"))?;
            Some((l, key))
        }
        None => None,
    };
    let app = App {
        cfg: Config {
            push_hosts: a.push_hosts,
            relay_free_bytes: a.relay_free_mb * 1_000_000,
            billing,
            origin: origin_of(&public_url)?,
            sites,
            also_redirect: a.also_redirect,
            block_domain,
            preview_domain,
            platform_host,
            platform_page: arugula_platform::page::Page::new(a.platform_static_dir),
            public_url,
            github,
            github_app_client,
            static_dir: a.static_dir,
        },
        db,
        http,
        relay: relay::Relay::new(relay::Caps {
            sockets: a.relay_max_sockets,
            daemons: a.relay_max_machines,
            daily_bytes: a.relay_daily_mb * 1_000_000,
            total: a.relay_max_total,
        }),
        passkeys: Default::default(),
        app_codes: Default::default(),
        sends: Default::default(),
        limits: limit::Limits::new(a.trust_proxy_header),
        vapid,
        hosted_key,
        github_app,
        forge: Default::default(),
        app_logins: Default::default(),
        daemon_sigs: Default::default(),
        old_hosts: Default::default(),
        jump,
        guest_routes: Default::default(),
        blocks: Default::default(),
        previews: Default::default(),
        analytics,
        // The database's directory, whose disk shared Docs fill. Without
        // one, the guard has no disk to read and lets every part in.
        docs: if a.no_disk_guard { Default::default() } else { doc_links::Docs::at(db_dir) },
        platform,
        maintenance: Default::default(),
        hub_port,
    }
    .start();
    if let Some((l, key)) = internal_listener {
        info!(addr = %l.local_addr()?, "the hub's internal API, for the platform");
        tokio::spawn(internal::serve(app.clone(), l, key));
    }
    // The maintenance switch, as the database says (M141).
    maintenance::start(app.clone());
    // Deleted accounts the platform hasn't erased yet (M140).
    account::start_telling(app.clone());
    if let (Some(l), Some(j)) = (guest_listener, &app.jump) {
        info!(addr = %l.local_addr()?, host = %j.host, port = j.port, "guest ssh jump host");
        tokio::spawn(guest_jump::serve(app.clone(), l));
    }
    tokio::spawn(relay::Relay::log_counts(app.clone()));
    if app.github_app.is_some() {
        tokio::spawn(forge::heartbeat(app.clone(), std::time::Duration::from_secs(a.forge_heartbeat_secs.max(1))));
    }
    // The platform's background work, when it's in process; out of
    // process its own binary runs it.
    if let Some(p) = app.platform.local() {
        arugula_platform::cloud::start(p);
        arugula_platform::hosted::start(p);
        arugula_platform::keys::start(p);
        // The job worker: runs' dispatch (M212) and schedules (M214).
        arugula_platform::work::start(p);
        // Chat follows the hub's change feed (M138), and so do projects and
        // grants: a machine that joins gets its `cloud.drive` grant from it.
        arugula_platform::chat::start(p);
        arugula_platform::projects::start(p);
        arugula_platform::billing::start(p);
        arugula_platform::forge::start(p);
    }
    // Shared Docs' open counts reach the database.
    {
        let a2 = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(doc_links::FLUSH_EVERY).await;
                a2.docs.flush(&a2.db);
            }
        });
    }
    // Expired sessions, join codes and invites go (#173).
    {
        let a2 = app.clone();
        tokio::spawn(async move {
            loop {
                // And the parts of a shared Doc's upload nobody finished.
                match a2.db.sweep_doc_links(arugula_e2e::now_ms().saturating_sub(doc_links::SWEEP_AFTER_MS)) {
                    Ok(n) if n > 0 => info!(parts = n, "swept uncommitted Doc parts"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "sweeping Doc parts"),
                }
                match a2.db.prune(arugula_e2e::now_ms()) {
                    Ok(n) if n > 0 => info!(sessions = n, "pruned expired sessions"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "pruning"),
                }
                // The platform's change feed keeps a week (`db::changes`).
                if let Err(e) = a2.db.prune_changes(arugula_e2e::now_ms()) {
                    tracing::warn!(error = %e, "pruning the change feed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    }
    // Nagle off: the relay's mux writes frames back to back (S15).
    let l = l.tap_io(|t| {
        let _ = t.set_nodelay(true);
    });
    info!(%listen, url = %app.cfg.public_url, "Arugula control");
    let serve = axum::serve(l, router(app.clone()).into_make_service_with_connect_info::<SocketAddr>());
    tokio::select! {
        r = serve => r?,
        _ = stopping() => info!("stopping"),
    }
    // An in-process platform's cloud jobs checkpoint before it goes (M144).
    if let Some(p) = app.platform.local() {
        p.drain(std::time::Duration::from_secs(5)).await;
    }
    app.analytics.flush().await;
    app.docs.flush(&app.db);
    // Litestream syncs what's left before control goes.
    if let Some(b) = &backup {
        b.stop().await;
    }
    Ok(())
}

/// Every relay connection is a socket: take all the open files the system
/// allows (#174), not the usual soft limit of 1024.
fn raise_open_files() {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: plain calls with a valid pointer to a local.
    unsafe {
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) != 0 {
            return;
        }
        if r.rlim_cur < r.rlim_max {
            let want = libc::rlimit { rlim_cur: r.rlim_max, rlim_max: r.rlim_max };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &want) == 0 {
                r = want;
            }
        }
    }
    info!(open_files = r.rlim_cur, "open file limit");
}

/// SIGTERM (Fly stopping the machine) or Ctrl-C.
async fn stopping() {
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term => {},
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn args() {
        use clap::CommandFactory;
        super::Args::command().debug_assert();
    }

    #[test]
    fn retired_run_caps_still_parse() {
        use clap::Parser;
        let a = super::Args::try_parse_from([
            "arugula-control",
            "--cloud-daily-runs",
            "10",
            "--cloud-concurrent-runs",
            "2",
            "--cloud-run-minutes",
            "30",
        ])
        .unwrap();
        assert_eq!(a.cloud.cloud_daily_runs.as_deref(), Some("10"));
        assert!(a.cloud.cloud_concurrent_runs.is_some() && a.cloud.cloud_run_minutes.is_some());
        let help = <super::Args as clap::CommandFactory>::command().render_help().to_string();
        assert!(!help.contains("daily-runs"), "{help}");
    }
}
