//! `arugula-platform`: control's platform as its own process (M140): chat,
//! Cloud, billing and the GitHub App's webhook (M161), behind the hub's gateway ("Platform paths stay on
//! control.arugula.io forever"). It has no public address. The hub
//! forwards each platform route here with a principal header signed with
//! the key both share (`ARUGULA_PLATFORM_KEY`), and the platform calls the
//! hub's internal API (`HubPort` over HTTP) with the same key. Without a
//! valid header it answers nothing but its health check. The platform page
//! (M151) is its too: the hub forwards the page's GETs on `app.arugula.io`
//! here, signed like the rest.
//!
//! `arugula-control --platform in-process` (the default) runs the same
//! code inside the hub instead; that's the rollback (docs/control-ops.md).
//!
//! The platform itself is `arugula-platform-base` (crates/platform) and the
//! feature crates on it (#314); this binary is its flags and its start.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context as _;
use arugula_control_common::{analytics::Analytics, principal::PrincipalKey};
use arugula_platform::{Config, Platform, PlatformPort, remote};
use axum::{Json, Router, routing::get};
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(version, about = "Arugula control's platform: chat, Cloud and billing, behind the hub")]
struct Args {
    /// Address to listen on. Only the hub calls it: on Fly that's the
    /// private network (`[::]:7700`), with no public port.
    #[arg(long, default_value = "127.0.0.1:7700", env = "ARUGULA_PLATFORM_LISTEN")]
    listen: SocketAddr,

    /// The platform's SQLite database, when ARUGULA_PLATFORM_DATABASE_URL
    /// doesn't name Postgres: the default for running your own and for
    /// development. Hosted control is on Postgres, with no volume (Arugula #72),
    /// and never opens it.
    #[arg(long, default_value = "platform.db", env = "ARUGULA_PLATFORM_DB")]
    db: PathBuf,

    /// Refuse to start unless ARUGULA_PLATFORM_DATABASE_URL names
    /// Postgres. Hosted control's `fly.toml` sets it: its machines have no
    /// volume, so a SQLite file there would be lost on the next restart.
    #[arg(long, env = "ARUGULA_PLATFORM_REQUIRE_POSTGRES", value_parser = clap::builder::FalseyValueParser::new())]
    require_postgres: bool,

    /// The hub's internal API (its `--internal-listen`), on the private
    /// network: `http://illogical-control.internal:7691` on Fly.
    #[arg(long, env = "ARUGULA_PLATFORM_HUB")]
    hub: String,

    /// The key the hub and the platform share (hex, at least 32 bytes):
    /// the hub signs principals and its calls here with it, and the
    /// platform its calls to the hub.
    #[arg(long, env = "ARUGULA_PLATFORM_KEY", hide_env_values = true)]
    key: String,

    /// The URL people reach control at (the hub's), without a trailing
    /// slash: where billing's checkout comes back to.
    #[arg(long, default_value = "http://127.0.0.1:7690", env = "ARUGULA_CONTROL_URL")]
    public_url: String,

    /// More URLs the hub answers at (its `--also-url`), comma-separated.
    #[arg(long, value_delimiter = ',', env = "ARUGULA_CONTROL_ALSO_URLS")]
    also_url: Vec<String>,

    /// The platform page's host (M151), as the hub's `--platform-host`:
    /// `app.arugula.io`. Links to the page (an issue's, a session's, a
    /// thread's, in prompts and PR bodies) are on it, with the public URL's
    /// scheme and port. Unset, they're on the public URL.
    #[arg(long, env = "ARUGULA_PLATFORM_HOST")]
    platform_host: Option<String>,

    /// Free relay traffic per account per month, in MB (with billing on):
    /// the same as the hub's.
    #[arg(long, default_value_t = 10_000, env = "ARUGULA_RELAY_FREE_MB")]
    relay_free_mb: u64,

    /// Hold background work at start (the job worker, hosted workspaces'
    /// worker, billing's usage reports, the projects and grants feed
    /// follower): for a window that copies the platform's tables (M141,
    /// M145). The variable is on unless it's empty, `0`, `false`, `no` or
    /// `off`, so the runbook's `ARUGULA_PLATFORM_PAUSED=1` holds it.
    #[arg(long, env = "ARUGULA_PLATFORM_PAUSED", value_parser = clap::builder::FalseyValueParser::new())]
    paused: bool,

    /// Postgres instead of `--db` (M143): `ARUGULA_PLATFORM_DATABASE_URL`.
    #[command(flatten)]
    database: arugula_platform::db::Args,

    /// The master key (ARUGULA_CLOUD_SECRET_KEY, and the others while it's
    /// rotated) and the beta allowlist (ARUGULA_CLOUD_ACCOUNTS).
    #[command(flatten)]
    cloud: arugula_platform::cloud::Args,

    /// Hosted workspaces' boxes (M176): ARUGULA_E2B_URL and
    /// ARUGULA_E2B_API_KEY.
    #[command(flatten)]
    compute: arugula_platform::compute::Args,

    /// Hosted workspaces (M179): ARUGULA_HOSTED_TEMPLATE,
    /// ARUGULA_HOSTED_ENROLL and ARUGULA_HOSTED_ACCOUNTS; and their
    /// inference (M185): ARUGULA_HOSTED_PLATFORM_KEYS and the turn caps.
    #[command(flatten)]
    hosted: arugula_platform::hosted::Args,

    /// The run-log bucket (M213): ARUGULA_RUN_LOGS_BUCKET, its endpoint,
    /// token and days.
    #[command(flatten)]
    run_logs: arugula_platform::run_logs::Args,

    #[command(flatten)]
    billing: arugula_platform::billing::Args,

    /// The Infisical projection (M235): ARUGULA_INFISICAL_PROJECTION, and
    /// the writer identity's address, project, environment and
    /// credentials.
    #[command(flatten)]
    infisical: arugula_platform::infisical::Args,

    /// The GitHub App (M161): its id, slug, private key and webhook secret,
    /// with the names they had on the hub.
    #[command(flatten)]
    github_app: arugula_platform::forge::Args,

    /// GitHub's API (tests point it at a fake).
    #[arg(long, default_value = "https://api.github.com", hide = true)]
    github_api: String,

    /// Product analytics: the same PostHog project as the hub's.
    #[arg(long, env = "POSTHOG_API_KEY", hide_env_values = true)]
    posthog_api_key: Option<String>,
    #[arg(long, default_value = "https://us.i.posthog.com", env = "POSTHOG_HOST")]
    posthog_host: String,

    /// Off-site backup of the SQLite file with Litestream (M141), as the
    /// hub's, under its own path in the bucket (`platform` unless
    /// LITESTREAM_PATH says). Never on Postgres.
    #[command(flatten)]
    backup: arugula_control_common::backup::Litestream,

    /// Serve the platform page from this directory instead of the built-in
    /// copy (development). The hub forwards the page's GETs on its
    /// `--platform-host` here (M151).
    #[arg(long)]
    static_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    // `import redone`: redone's tenants, tokens and apps (M235). Any other
    // `import` falls through to `import planter` below.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "import") && args.get(1).is_some_and(|w| w == "redone") {
        return arugula_platform::redone::run("arugula-platform import", args.into_iter().skip(1));
    }
    // `copy`, `checksums`, `secrets-check`, `migrations`: a window's tools
    // (window.rs, M141, M145), run on the platform's machine beside the
    // server, and by the nightly dump. `usage`: hosted workspaces' usage
    // and cost (M188), read-only. `rotate-key`: the master key's rotation
    // (M209, `keys::rotate`). `previews`: public previews, and the
    // operator's takedown (M187). `run-logs`: runs' logs and artifacts in
    // the run-log bucket, by project (M213), read-only. `stuck`: runs and
    // deploys that are stuck, and unsticking one (M223).
    // `import planter`: planter's repos as repo links (M232,
    // `arugula_platform::planter`), with the server's configuration.
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().is_some_and(|a| a == "import") {
        argv.next();
        return arugula_platform::planter::run("arugula-platform import", argv);
    }
    if argv.peek().is_some_and(|a| {
        ["copy", "checksums", "secrets-check", "migrations", "usage", "rotate-key", "previews", "run-logs", "stuck"]
            .contains(&a.as_str())
    }) {
        return arugula_platform::window::run("arugula-platform", argv);
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

/// The platform page's origin on `host`, with `public_url`'s scheme and
/// port (as the hub's `platform_host::parse` makes it); "" for no host.
fn page_origin(public_url: &str, host: &str) -> anyhow::Result<String> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return Ok(String::new());
    }
    let mut u = url::Url::parse(public_url).context("ARUGULA_CONTROL_URL")?;
    u.set_host(Some(host)).map_err(|e| anyhow::anyhow!("ARUGULA_PLATFORM_HOST {host:?}: {e}"))?;
    Ok(u.origin().ascii_serialization())
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "arugula_platform=info".into()),
        )
        .init();
    let a = Args::parse();
    let key = PrincipalKey::from_hex(&a.key).context("ARUGULA_PLATFORM_KEY")?;
    let l = tokio::net::TcpListener::bind(a.listen).await.with_context(|| format!("listening on {}", a.listen))?;
    let public_url = a.public_url.trim_end_matches('/').to_owned();
    let page_url = page_origin(&public_url, a.platform_host.as_deref().unwrap_or(""))?;
    // The page's origin is one of ours too (chat's previews, #108), as in
    // process.
    let sites = std::iter::once(public_url.clone())
        .chain(a.also_url.iter().filter(|u| !u.is_empty()).map(|u| u.trim_end_matches('/').to_owned()))
        .chain(Some(page_url.clone()).filter(|u| !u.is_empty()))
        .collect();
    let cfg = Config { public_url, sites, page_url, relay_free_bytes: a.relay_free_mb * 1_000_000 };
    let http = arugula_control_common::roots::client(std::time::Duration::from_secs(15))?;
    let analytics = match a.posthog_api_key.clone().filter(|k| !k.is_empty()) {
        Some(k) => Analytics::start(k, &a.posthog_host, http.clone()),
        None => {
            info!("no POSTHOG_API_KEY: product analytics are off");
            Analytics::default()
        }
    };
    let hub = Arc::new(remote::HttpHub::new(&a.hub, key.clone())?);
    // The hub's backup is under `control`: two databases replicating to one
    // path would ruin both.
    anyhow::ensure!(a.backup.path.as_deref() != Some("control"), "LITESTREAM_PATH=control is the hub's backup");
    let backup = backup(&a)?;
    if let Some(b) = &backup {
        b.restore_if_missing().await?;
    }
    let db =
        a.database.open(&a.db).with_context(|| format!("opening the platform's database (else {})", a.db.display()))?;
    if let Some(b) = &backup {
        b.replicate();
    }
    let mut p = Platform::new(cfg, db, hub, http, analytics);
    p.stripe = a.billing.stripe()?;
    p.hosted = a.hosted.config(&a.cloud)?;
    p.secret_key = p.hosted.cipher.clone();
    p.compute = a.compute.substrate()?;
    p.github = a.github_app.app(&a.github_api)?;
    p.deploy_app = a.github_app.deploy_app(&a.github_api)?;
    p.run_logs = a.run_logs.bucket()?;
    p.projection = a.infisical.projection(&p.http)?;
    if p.projection.mode != arugula_platform::infisical::Mode::Off {
        info!(mode = ?p.projection.mode, "the Infisical projection");
    }
    p.paused = a.paused.into();
    let p = Arc::new(p);
    if a.paused {
        info!("paused: no job worker, hosted worker, usage reports or grant follower until restarted without it");
    }
    arugula_platform::hosted::start(&p);
    arugula_platform::keys::start(&p);
    // Clients' secret events past their days (M236).
    arugula_platform::clients::start(&p);
    // The job worker: runs' dispatch (M212) and schedules (M214).
    arugula_platform::work::start(&p);
    // Runs' logs past their days (M213).
    arugula_platform::run_logs::start(&p);
    // Chat follows the hub's change feed, and so do projects and grants (a
    // machine that joins gets its `cloud.drive` grant from it).
    arugula_platform::chat::start(&p);
    arugula_platform::projects::start(&p);
    arugula_platform::billing::start(&p);
    arugula_platform::forge::start(&p);

    let port: Arc<dyn PlatformPort> = p.clone();
    let app = Router::new()
        .merge(
            arugula_platform::router(p.clone())
                .layer(axum::middleware::from_fn_with_state(key.clone(), remote::vouched)),
        )
        .merge(remote::serve_platform(port, key.clone()))
        // Fly's health check: the only thing answered without the hub. A
        // machine that can't reach its database answers 503, so a rolling
        // deploy (M144: two machines) stops at it rather than going on.
        .route("/platform.json", get(health).with_state(p.clone()))
        // Anything else is the platform page (M151), for the page GETs the
        // hub forwards from its platform host, and only for those.
        .merge(arugula_platform::page::behind_hub(arugula_platform::page::Page::new(a.static_dir), key));
    info!(listen = %l.local_addr()?, hub = %a.hub, "Arugula platform");
    let serve = axum::serve(l, app);
    tokio::select! {
        r = serve => r?,
        _ = stopping() => info!("stopping"),
    }
    // Cloud jobs this machine holds go back to the other (or to the next
    // start) as soon as they checkpoint, not when their leases run out.
    p.drain(std::time::Duration::from_secs(10)).await;
    p.analytics.flush().await;
    // Litestream syncs what's left before the platform goes.
    if let Some(b) = &backup {
        b.stop().await;
    }
    Ok(())
}

/// Litestream for the SQLite file, or nothing on Postgres, which has its
/// own backups (M145's nightly dump). On Postgres it touches nothing on
/// disk: hosted control's machines have no `/data` (Arugula #72).
fn backup(a: &Args) -> anyhow::Result<Option<arugula_control_common::backup::Backup>> {
    if a.database.postgres() {
        return Ok(None);
    }
    anyhow::ensure!(
        !a.require_postgres,
        "ARUGULA_PLATFORM_REQUIRE_POSTGRES is set but ARUGULA_PLATFORM_DATABASE_URL doesn't name Postgres: \
         this platform has no volume for a SQLite file (docs/control-ops.md, \"The platform\")"
    );
    arugula_control_common::backup::Backup::new(&a.backup, &a.db, "platform")
}

/// `GET /platform.json`: up, its version, its database answering, and
/// whether it's the platform's leader (M144).
async fn health(axum::extract::State(p): axum::extract::State<Arc<Platform>>) -> axum::response::Response {
    use arugula_platform::db::Sql as _;
    use axum::response::IntoResponse as _;
    let db = p.db.c().query_row("SELECT 1", &[], |r| r.get::<i64>(0));
    let body = serde_json::json!({
        "platform": true,
        "version": env!("CARGO_PKG_VERSION"),
        "database": db.is_ok(),
        "leader": p.is_leader(),
    });
    match db {
        Ok(_) => Json(body).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "health check: the database didn't answer");
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
        }
    }
}

/// SIGTERM (Fly stopping the machine) or Ctrl-C. Windows has only Ctrl-C:
/// the crate builds there with the rest of the workspace.
async fn stopping() {
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
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

    /// The page's origin is the platform host with control's scheme and
    /// port, as the hub makes it: `https://app.arugula.io`.
    #[test]
    fn the_page_is_on_its_host() {
        let o = |public: &str, host: &str| super::page_origin(public, host).unwrap();
        assert_eq!(o("https://control.arugula.io", "app.arugula.io"), "https://app.arugula.io");
        assert_eq!(o("http://10.0.0.10:8080", "app.control.test."), "http://app.control.test:8080");
        assert_eq!(o("https://control.arugula.io", ""), "", "none: links stay on control's URL");
        let cfg = |page: &str| arugula_platform::Config {
            public_url: "https://control.arugula.io".into(),
            page_url: page.into(),
            ..Default::default()
        };
        assert_eq!(cfg("https://app.arugula.io").page(), "https://app.arugula.io");
        assert_eq!(cfg("").page(), "https://control.arugula.io");
    }

    /// `ARUGULA_PLATFORM_PAUSED=1`, as the runbook and production's staged
    /// secret say, holds the platform. Parsed as a plain bool it was
    /// refused (`true` or `false` only), and the platform didn't start.
    #[test]
    fn paused_takes_one_and_zero() {
        use clap::Parser;
        let parse = |v: Option<&str>| {
            // SAFETY: the only test here that sets or reads the variable.
            unsafe {
                match v {
                    Some(v) => std::env::set_var("ARUGULA_PLATFORM_PAUSED", v),
                    None => std::env::remove_var("ARUGULA_PLATFORM_PAUSED"),
                }
            }
            super::Args::try_parse_from(["arugula-platform", "--hub", "http://hub", "--key", "00"]).unwrap().paused
        };
        for (v, want) in [("1", true), ("true", true), ("yes", true), ("0", false), ("false", false), ("", false)] {
            assert_eq!(parse(Some(v)), want, "{v:?}");
        }
        assert!(!parse(None));
        let flag = super::Args::try_parse_from(["arugula-platform", "--hub", "http://hub", "--key", "00", "--paused"]);
        assert!(flag.unwrap().paused);
    }

    /// Hosted control (Arugula #72): on Postgres the platform needs no `/data`.
    /// With Litestream's bucket set (the nightly dump reads the app's
    /// `LITESTREAM_*`) and `--db` under a directory that doesn't exist, it
    /// still starts no backup and writes nothing there. Without Postgres,
    /// `--require-postgres` stops it rather than start on a file.
    #[test]
    fn postgres_needs_no_volume() {
        use clap::Parser;
        let dir = std::env::temp_dir().join(format!("arugula-platform-no-data-{}", std::process::id()));
        let db = dir.join("platform.db");
        let db = db.to_str().unwrap();
        let base = ["arugula-platform", "--hub", "http://hub", "--key", "00", "--litestream-bucket", "b", "--db", db];
        let pg = super::Args::try_parse_from(base.iter().copied().chain([
            "--platform-database-url",
            "postgresql://u@db.flycast/p",
            "--require-postgres",
        ]))
        .unwrap();
        assert!(super::backup(&pg).unwrap().is_none());
        assert!(!dir.exists(), "nothing is written beside --db on Postgres");
        let file = super::Args::try_parse_from(base.iter().copied().chain(["--require-postgres"])).unwrap();
        let err = super::backup(&file).err().expect("refused without Postgres").to_string();
        assert!(err.contains("ARUGULA_PLATFORM_REQUIRE_POSTGRES"), "{err}");
        assert!(!dir.exists());
    }
}
