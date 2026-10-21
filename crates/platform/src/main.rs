//! `arugula-platform`: control's platform as its own process (M140): chat,
//! Cloud and billing, behind the hub's gateway ("Platform paths stay on
//! control.arugula.io forever"). It has no public address. The hub
//! forwards each platform route here with a principal header signed with
//! the key both share (`ARUGULA_PLATFORM_KEY`), and the platform calls the
//! hub's internal API (`HubPort` over HTTP) with the same key. Without a
//! valid header it answers nothing but its health check.
//!
//! `arugula-control --platform in-process` (the default) runs the same
//! code inside the hub instead; that's the rollback (docs/control-ops.md).

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

    /// The platform's own SQLite database. Until M141 moves the platform's
    /// tables into it, it starts empty.
    #[arg(long, default_value = "platform.db", env = "ARUGULA_PLATFORM_DB")]
    db: PathBuf,

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

    /// Free relay traffic per account per month, in MB (with billing on):
    /// the same as the hub's.
    #[arg(long, default_value_t = 10_000, env = "ARUGULA_RELAY_FREE_MB")]
    relay_free_mb: u64,

    /// Hold background work at start (the cloud worker, Cloud's approval
    /// pushes, billing's usage reports, the projects and grants feed
    /// follower): for a window that copies the platform's tables (M141,
    /// M145). The variable is on unless it's empty, `0`, `false`, `no` or
    /// `off`, so the runbook's `ARUGULA_PLATFORM_PAUSED=1` holds it.
    #[arg(long, env = "ARUGULA_PLATFORM_PAUSED", value_parser = clap::builder::FalseyValueParser::new())]
    paused: bool,

    #[command(flatten)]
    cloud: arugula_platform::cloud::Args,

    #[command(flatten)]
    billing: arugula_platform::billing::Args,

    /// Product analytics: the same PostHog project as the hub's.
    #[arg(long, env = "POSTHOG_API_KEY", hide_env_values = true)]
    posthog_api_key: Option<String>,
    #[arg(long, default_value = "https://us.i.posthog.com", env = "POSTHOG_HOST")]
    posthog_host: String,

    /// Off-site backup with Litestream (M141), as the hub's, under its own
    /// path in the bucket (`platform` unless LITESTREAM_PATH says).
    #[command(flatten)]
    backup: arugula_control_common::backup::Litestream,
}

fn main() -> anyhow::Result<()> {
    // `copy`, `checksums`, `secrets-check`: a window's tools (window.rs,
    // M141), run on the platform's machine beside the server.
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().is_some_and(|a| ["copy", "checksums", "secrets-check"].contains(&a.as_str())) {
        return arugula_platform::window::run("arugula-platform", argv);
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
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
    let sites = std::iter::once(public_url.clone())
        .chain(a.also_url.iter().filter(|u| !u.is_empty()).map(|u| u.trim_end_matches('/').to_owned()))
        .collect();
    let cfg = Config { public_url, sites, relay_free_bytes: a.relay_free_mb * 1_000_000 };
    let http = arugula_control_common::roots::client(std::time::Duration::from_secs(15))?;
    let analytics = match a.posthog_api_key.filter(|k| !k.is_empty()) {
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
    let backup = arugula_control_common::backup::Backup::new(&a.backup, &a.db, "platform")?;
    if let Some(b) = &backup {
        b.restore_if_missing().await?;
    }
    let db = arugula_platform::db::Db::open(&a.db).with_context(|| format!("opening {}", a.db.display()))?;
    if let Some(b) = &backup {
        b.replicate();
    }
    let mut p = Platform::new(cfg, db, hub, http, analytics);
    p.stripe = a.billing.stripe()?;
    p.cloud = a.cloud.cloud()?;
    p.paused = a.paused.into();
    let p = Arc::new(p);
    if a.paused {
        info!("paused: no cloud worker, approval pushes, usage reports or grant follower until restarted without it");
    }
    arugula_platform::cloud::start(&p);
    // Chat follows the hub's change feed, and so do projects and grants (a
    // machine that joins gets its `cloud.drive` grant from it).
    arugula_platform::chat::start(&p);
    arugula_platform::projects::start(&p);
    arugula_platform::billing::start(&p);

    let port: Arc<dyn PlatformPort> = p.clone();
    let app = Router::new()
        .merge(
            arugula_platform::router(p.clone())
                .layer(axum::middleware::from_fn_with_state(key.clone(), remote::vouched)),
        )
        .merge(remote::serve_platform(port, key))
        // Fly's health check: the only thing answered without the hub.
        .route(
            "/platform.json",
            get(|| async { Json(serde_json::json!({ "platform": true, "version": env!("CARGO_PKG_VERSION") })) }),
        );
    info!(listen = %l.local_addr()?, hub = %a.hub, "Arugula platform");
    let serve = axum::serve(l, app);
    tokio::select! {
        r = serve => r?,
        _ = stopping() => info!("stopping"),
    }
    p.analytics.flush().await;
    // Litestream syncs what's left before the platform goes.
    if let Some(b) = &backup {
        b.stop().await;
    }
    Ok(())
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
}
