//! The maintenance switch (M141, #1158): for an announced window, every
//! platform route (each `process = "platform"` route in `routes.toml`:
//! chat, Cloud, billing) answers
//!
//! `503`, `Retry-After: N`, `{"error": "Chat and Cloud are down for maintenance; try again in N seconds.", "reason": "maintenance"}`
//!
//! before anything reaches the platform, in process or out (`gateway`).
//! Terminals, the relay and every other hub route are untouched. Clients
//! retry a 503 with reason `maintenance` whatever its method
//! (`arugula_control_wire::maintenance`): it was never processed. The
//! error text is what older daemons' agents show as it is, so it says
//! what to do.
//!
//! The operator turns it on and off on the hub's machine, which restarts
//! nothing (the relay stays up):
//!
//! ```text
//! fly ssh console -a illogical-control -C "/arugula-control maintenance on --retry-after 30 --pause --db /data/control.db"
//! fly ssh console -a illogical-control -C "/arugula-control maintenance off --db /data/control.db"
//! ```
//!
//! It's a row in the hub's database (`db::platform`), read every second,
//! so it holds across a hub restart. `--pause` also holds the in-process
//! platform's background work (the cloud worker, approval pushes, usage
//! reports, the projects and grants feed follower), so its tables hold
//! still while a window copies them; an out-of-process platform is held
//! with its own `ARUGULA_PLATFORM_PAUSED`.

use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::Context as _;
use arugula_control_wire::maintenance::MAINTENANCE;
use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use clap::Parser;
use tracing::{info, warn};

use crate::{App, db::platform::Maintenance};

/// How often the running hub reads the switch.
const READ_EVERY: Duration = Duration::from_secs(1);

/// What `Retry-After` says when the operator doesn't say.
const DEFAULT_RETRY_AFTER: u64 = 30;

/// The switch as the hub last read it.
#[derive(Default)]
pub struct Switch(RwLock<Option<Maintenance>>);

impl Switch {
    /// `Retry-After`'s seconds while it's on.
    pub fn retry_after(&self) -> Option<u64> {
        self.0.read().unwrap().map(|m| m.retry_after)
    }

    fn set(&self, m: Option<Maintenance>) -> Option<Maintenance> {
        std::mem::replace(&mut *self.0.write().unwrap(), m)
    }
}

/// The answer to every platform route during a window, `n` seconds.
pub fn answer(n: u64) -> Response {
    let mut res = crate::refusal(
        StatusCode::SERVICE_UNAVAILABLE,
        MAINTENANCE,
        &format!("Chat and Cloud are down for maintenance; try again in {n} seconds."),
    )
    .into_response();
    res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(n));
    res
}

/// In front of the platform's routes: while the switch is on, nothing
/// reaches the platform.
pub async fn refuse(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    if let Some(n) = app.maintenance.retry_after() {
        return answer(n);
    }
    next.run(req).await
}

/// Read the switch from the database now: what the running hub does every
/// second.
pub fn refresh(app: &App) -> anyhow::Result<()> {
    let now = app.db.maintenance()?;
    let was = app.maintenance.set(now);
    if let Some(p) = app.platform.local() {
        p.paused.store(now.is_some_and(|m| m.pause), std::sync::atomic::Ordering::Relaxed);
    }
    match (was, now) {
        (None, Some(m)) => {
            info!(retry_after = m.retry_after, pause = m.pause, "maintenance on: platform routes answer 503")
        }
        (Some(_), None) => info!("maintenance off"),
        (Some(a), Some(b)) if a != b => info!(retry_after = b.retry_after, pause = b.pause, "maintenance changed"),
        _ => {}
    }
    Ok(())
}

/// Read the switch every [`READ_EVERY`].
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = refresh(&app) {
                warn!(error = %e, "reading the maintenance switch");
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    });
}

#[derive(Parser, Debug)]
#[command(
    name = "arugula-control maintenance",
    about = "Turn the maintenance switch on or off: platform routes answer 503 while it's on"
)]
struct Args {
    #[command(subcommand)]
    what: What,
    /// Control's database, which the running hub reads the switch from.
    #[arg(long, global = true, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// Platform routes answer 503 from now on (within a second).
    On {
        /// What `Retry-After` and the error say, in seconds.
        #[arg(long, default_value_t = DEFAULT_RETRY_AFTER)]
        retry_after: u64,
        /// Also hold the in-process platform's background work: the cloud
        /// worker, approval pushes, usage reports and the grant follower.
        #[arg(long)]
        pause: bool,
    },
    /// Platform routes are answered again.
    Off,
    /// Whether it's on.
    Status,
}

/// `arugula-control maintenance on|off|status`.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control maintenance".to_owned()).chain(args));
    // The hub's file, never a new one.
    let c = rusqlite::Connection::open_with_flags(
        &a.db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", a.db.display()))?;
    c.busy_timeout(Duration::from_secs(5))?;
    match a.what {
        What::On { retry_after, pause } => {
            anyhow::ensure!((1..=3600).contains(&retry_after), "--retry-after is 1 to 3600 seconds");
            let m = Maintenance { retry_after, pause, since: arugula_e2e::now_ms() };
            crate::db::platform::set_maintenance(&c, Some(m))?;
            println!(
                "maintenance on: platform routes answer 503 with Retry-After {retry_after} within a second{}",
                if pause { "; the in-process platform's background work is held" } else { "" }
            );
        }
        What::Off => {
            crate::db::platform::set_maintenance(&c, None)?;
            println!("maintenance off: platform routes are answered again within a second");
        }
        What::Status => match crate::db::platform::maintenance(&c)? {
            Some(m) => println!(
                "maintenance on since {} (ms): Retry-After {}{}",
                m.since,
                m.retry_after,
                if m.pause { ", background work held" } else { "" }
            ),
            None => println!("maintenance off"),
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// During a window every platform route answers the 503 clients wait
    /// out, with the same N in the header and the sentence, and nothing
    /// else changes; off again, it's answered.
    #[tokio::test]
    async fn a_window_answers_platform_routes_and_only_those() {
        let app = App::for_tests("http://control.test").start();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let a2 = app.clone();
        tokio::spawn(async move {
            axum::serve(l, crate::router(a2).into_make_service_with_connect_info::<std::net::SocketAddr>()).await
        });
        let http = reqwest::Client::new();
        let get = |p: &str| http.get(format!("{base}{p}")).send();

        assert_eq!(get("/api/chat").await.unwrap().status(), 401, "answered by the platform");
        app.db.set_maintenance(Some(Maintenance { retry_after: 42, pause: true, since: 1 })).unwrap();
        refresh(&app).unwrap();
        assert!(app.platform.inner().is_paused(), "--pause holds the in-process platform's work");
        for (m, p) in [("GET", "/api/chat"), ("POST", "/api/cloud/workspaces"), ("POST", "/api/stripe/webhook")] {
            let r = http.request(m.parse().unwrap(), format!("{base}{p}")).send().await.unwrap();
            assert_eq!(r.status(), 503, "{m} {p}");
            assert_eq!(r.headers()["retry-after"], "42");
            let body: serde_json::Value = r.json().await.unwrap();
            assert_eq!(
                body,
                serde_json::json!({
                    "error": "Chat and Cloud are down for maintenance; try again in 42 seconds.",
                    "reason": "maintenance"
                })
            );
        }
        assert_eq!(get("/control.json").await.unwrap().status(), 200, "the hub's own routes are answered");
        assert_eq!(get("/api/devices").await.unwrap().status(), 401, "and still checked as ever");
        app.db.set_maintenance(None).unwrap();
        refresh(&app).unwrap();
        assert!(!app.platform.inner().is_paused());
        assert_eq!(get("/api/chat").await.unwrap().status(), 401, "answered again");
    }

    /// The operator's command sets the row the hub reads, in the hub's
    /// own file, and makes no file of its own.
    #[test]
    fn the_command_sets_the_switch() {
        let dir = std::env::temp_dir().join(format!("arugula-maint-{}", hex::encode(arugula_e2e::random::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.db");
        let cmd = |args: &[&str]| {
            run(args.iter().map(|s| (*s).to_owned()).chain(["--db".into(), path.display().to_string()]))
        };
        assert!(cmd(&["status"]).is_err(), "no database there");
        assert!(!path.exists());
        let db = crate::db::Db::open(&path).unwrap();
        cmd(&["on", "--retry-after", "20", "--pause"]).unwrap();
        let m = db.maintenance().unwrap().unwrap();
        assert_eq!((m.retry_after, m.pause), (20, true));
        assert!(cmd(&["on", "--retry-after", "0"]).is_err());
        cmd(&["off"]).unwrap();
        assert_eq!(db.maintenance().unwrap(), None);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
