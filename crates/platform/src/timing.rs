//! Where a platform request's time goes, as a `Server-Timing` header
//! (platform step 2, #1150: production tells us, not a guess). Every
//! platform route answers with
//!
//! ```text
//! Server-Timing: db;dur=1.84, dbq;desc=3, hub;dur=0.00, hubn;desc=0, app;dur=2.31
//! ```
//!
//! - `db`: time in database calls, waiting for a connection included (on
//!   Postgres, a pool slot; on SQLite, its lock), in ms.
//! - `dbq`: statements sent. On Postgres each is a round trip
//!   (`BEGIN` and `COMMIT` too). `dbprep` (statements prepared: one more
//!   round trip each) and `dbconn` (connections opened) are only there when
//!   not zero: in steady state both are.
//! - `hub`, `hubn`: time in `HubPort` calls and how many, the change-feed
//!   catch-up for `x-arugula-hub-seq` included ([`crate::cache::fresh`]).
//!   Out of process each is a round trip to the hub.
//! - `app`: the whole request, from the platform's router until its answer
//!   (an answer's head, for a stream).
//!
//! The hub's gateway passes these on and adds its own (`up`, `gw`;
//! `arugula-control`'s `gateway.rs`). Only durations and counts: never a
//! path, an id or anything a request carried. `docs/control-ops.md`
//! ("Where a request's time goes") says how to read it.
//!
//! The numbers are the request's task's ([`tokio::task_local!`]): a
//! database or hub call made from another task (a spawned one, the
//! background work) isn't counted against a request.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{MatchedPath, Request},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use futures_util::future::BoxFuture;

use crate::hub::{Account, ChangePage, Daemon, Flags, HubPort, HubResult, Meter, PushOptions, Pushed, Resolved, Team};

/// The header's name.
pub const HEADER: &str = "server-timing";

/// One request's counts, in µs where they're times.
#[derive(Default, Debug)]
pub struct Timing {
    db_us: AtomicU64,
    dbq: AtomicU64,
    dbprep: AtomicU64,
    dbconn: AtomicU64,
    hub_us: AtomicU64,
    hubn: AtomicU64,
}

/// What [`Timing`] counted, read out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub db: Duration,
    pub dbq: u64,
    pub dbprep: u64,
    pub dbconn: u64,
    pub hub: Duration,
    pub hubn: u64,
}

impl Timing {
    pub fn counts(&self) -> Counts {
        Counts {
            db: Duration::from_micros(self.db_us.load(Relaxed)),
            dbq: self.dbq.load(Relaxed),
            dbprep: self.dbprep.load(Relaxed),
            dbconn: self.dbconn.load(Relaxed),
            hub: Duration::from_micros(self.hub_us.load(Relaxed)),
            hubn: self.hubn.load(Relaxed),
        }
    }
}

tokio::task_local! {
    static NOW: Arc<Timing>;
}

fn with(f: impl FnOnce(&Timing)) {
    let _ = NOW.try_with(|t| f(t));
}

fn us(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

/// One statement: timed and counted against the request it's for.
pub fn statement<T>(f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    let took = us(t.elapsed());
    with(|x| {
        x.db_us.fetch_add(took, Relaxed);
        x.dbq.fetch_add(1, Relaxed);
    });
    out
}

/// Waiting for a connection: timed, not counted.
pub fn waited<T>(f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    let took = us(t.elapsed());
    with(|x| {
        x.db_us.fetch_add(took, Relaxed);
    });
    out
}

/// A statement was prepared (a round trip of its own, inside a
/// [`statement`]'s time).
pub fn prepared() {
    with(|x| {
        x.dbprep.fetch_add(1, Relaxed);
    });
}

/// A connection was opened (inside a [`waited`]'s time).
pub fn connected() {
    with(|x| {
        x.dbconn.fetch_add(1, Relaxed);
    });
}

/// One hub call: timed and counted, also when it's given up on before it
/// answers (`cache::fresh`'s wait).
pub async fn hub<F: Future>(f: F) -> F::Output {
    struct Call(Instant);
    impl Drop for Call {
        fn drop(&mut self) {
            let took = us(self.0.elapsed());
            with(|x| {
                x.hub_us.fetch_add(took, Relaxed);
                x.hubn.fetch_add(1, Relaxed);
            });
        }
    }
    let _call = Call(Instant::now());
    f.await
}

/// Run `f` with its own counts, and give them back with its output (tests
/// and benches; the middleware is [`layer`]).
pub async fn counted<F: Future>(f: F) -> (F::Output, Counts) {
    let t = Arc::new(Timing::default());
    let out = NOW.scope(t.clone(), f).await;
    (out, t.counts())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// The header's value for these counts and the request's whole time.
pub fn header(c: &Counts, app: Duration) -> String {
    let mut s = format!(
        "db;dur={:.2}, dbq;desc={}, hub;dur={:.2}, hubn;desc={}, app;dur={:.2}",
        ms(c.db),
        c.dbq,
        ms(c.hub),
        c.hubn,
        ms(app)
    );
    if c.dbprep > 0 {
        s += &format!(", dbprep;desc={}", c.dbprep);
    }
    if c.dbconn > 0 {
        s += &format!(", dbconn;desc={}", c.dbconn);
    }
    s
}

/// The platform router's outermost layer: counts the request's database and
/// hub calls, answers with them in [`HEADER`], and logs them at `debug`
/// with the route's pattern (never its ids).
pub async fn layer(req: Request, next: Next) -> Response {
    let route = req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_owned()).unwrap_or_default();
    let method = req.method().clone();
    let t = Arc::new(Timing::default());
    let started = Instant::now();
    let mut res = NOW.scope(t.clone(), next.run(req)).await;
    let app = started.elapsed();
    let c = t.counts();
    let value = header(&c, app);
    tracing::debug!(
        %method,
        route,
        status = res.status().as_u16(),
        app_ms = ms(app),
        db_ms = ms(c.db),
        dbq = c.dbq,
        dbprep = c.dbprep,
        dbconn = c.dbconn,
        hub_ms = ms(c.hub),
        hubn = c.hubn,
        "platform request"
    );
    if let Ok(v) = HeaderValue::from_str(&value) {
        res.headers_mut().append(HEADER, v);
    }
    res
}

/// A `HubPort` whose every call is timed and counted ([`hub`]): what
/// [`Platform::new`](crate::Platform::new) wraps the hub in.
pub struct Timed(pub Arc<dyn HubPort>);

macro_rules! timed {
    ($call:expr) => {
        Box::pin(hub($call))
    };
}

impl HubPort for Timed {
    fn resolve<'a>(&'a self, p: &'a arugula_control_common::Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        timed!(self.0.resolve(p))
    }
    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        timed!(self.0.accounts(ids))
    }
    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        timed!(self.0.accounts_called(word))
    }
    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        timed!(self.0.teams_of(account))
    }
    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        timed!(self.0.teams(ids))
    }
    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        timed!(self.0.daemons(ids))
    }
    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        timed!(self.0.machines_for(account))
    }
    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        timed!(self.0.may_reach(account, machines))
    }
    fn push<'a>(&'a self, account: &'a str, message: &'a serde_json::Value) -> BoxFuture<'a, HubResult<Pushed>> {
        timed!(self.0.push(account, message))
    }
    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a serde_json::Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>> {
        timed!(self.0.push_with(account, message, options))
    }
    fn labs_flags<'a>(
        &'a self,
        owners: &'a [String],
    ) -> BoxFuture<'a, HubResult<std::collections::BTreeMap<String, Flags>>> {
        timed!(self.0.labs_flags(owners))
    }
    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        timed!(self.0.meters(accounts, since, until))
    }
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        timed!(self.0.metered_accounts())
    }
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        timed!(self.0.changes(after, limit))
    }
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        timed!(self.0.personal_project(account))
    }
    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        timed!(self.0.can_drive_cloud(daemon, account))
    }
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        timed!(self.0.team_project(team))
    }
    fn forge_pokes<'a>(&'a self, pokes: &'a [arugula_control_wire::forge::Poke]) -> BoxFuture<'a, HubResult<u32>> {
        timed!(self.0.forge_pokes(pokes))
    }
    fn github_user<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<crate::forge::GithubUser>>> {
        timed!(self.0.github_user(account))
    }
    fn send_authorization<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        back: &'a str,
    ) -> BoxFuture<'a, HubResult<String>> {
        timed!(self.0.send_authorization(account, draft, back))
    }
    fn daemon_op<'a>(
        &'a self,
        daemon: &'a str,
        op: &'a arugula_control_wire::forge::DaemonOp,
    ) -> BoxFuture<'a, HubResult<arugula_control_wire::forge::OpAnswer>> {
        timed!(self.0.daemon_op(daemon, op))
    }
    fn hosted_enrollment<'a>(
        &'a self,
        ask: &'a arugula_control_wire::hosted::EnrollAsk,
    ) -> BoxFuture<'a, HubResult<arugula_control_wire::hosted::EnrollGiven>> {
        timed!(self.0.hosted_enrollment(ask))
    }
    fn hosted_enrolled<'a>(
        &'a self,
        workspaces: &'a [String],
    ) -> BoxFuture<'a, HubResult<Vec<arugula_control_wire::hosted::Enrolled>>> {
        timed!(self.0.hosted_enrolled(workspaces))
    }
    fn hosted_gone<'a>(&'a self, workspaces: &'a [String]) -> BoxFuture<'a, HubResult<()>> {
        timed!(self.0.hosted_gone(workspaces))
    }
    fn activity<'a>(&'a self, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<crate::hub::MachineActivity>>> {
        timed!(self.0.activity(machines))
    }
    fn live_delegation<'a>(&'a self, project: &'a str, owner: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        timed!(self.0.live_delegation(project, owner))
    }
    fn owner_browser_signed<'a>(
        &'a self,
        owner: &'a str,
        device: &'a str,
        body: &'a str,
        sig: &'a str,
    ) -> BoxFuture<'a, HubResult<bool>> {
        timed!(self.0.owner_browser_signed(owner, device, body, sig))
    }

    fn device_trust<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<crate::hub::DeviceTrust>>> {
        timed!(self.0.device_trust(account))
    }
    fn team_rosters<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Vec<arugula_e2e::team::Roster>>> {
        timed!(self.0.team_rosters(team))
    }
    fn verify_passkey<'a>(
        &'a self,
        account: &'a str,
        challenge: &'a str,
        assertion: &'a crate::hub::PasskeyAssertion,
    ) -> BoxFuture<'a, HubResult<crate::hub::PasskeyVerdict>> {
        timed!(self.0.verify_passkey(account, challenge, assertion))
    }
    fn product_links<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<crate::hub::ProductLink>>> {
        timed!(self.0.product_links(account))
    }
    fn unlink_product<'a>(&'a self, account: &'a str, client: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        timed!(self.0.unlink_product(account, client))
    }
    fn linked_account<'a>(
        &'a self,
        client: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, HubResult<Option<crate::hub::LinkedAccount>>> {
        timed!(self.0.linked_account(client, user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_says_durations_and_counts_only() {
        let c = Counts { db: Duration::from_micros(1840), dbq: 3, dbprep: 0, dbconn: 0, hub: Duration::ZERO, hubn: 0 };
        assert_eq!(
            header(&c, Duration::from_micros(2310)),
            "db;dur=1.84, dbq;desc=3, hub;dur=0.00, hubn;desc=0, app;dur=2.31"
        );
        let c = Counts { dbprep: 2, dbconn: 1, ..c };
        assert!(header(&c, Duration::ZERO).ends_with(", dbprep;desc=2, dbconn;desc=1"));
    }

    #[tokio::test]
    async fn counts_are_the_tasks() {
        let ((), c) = counted(async {
            statement(|| ());
            statement(|| ());
            waited(|| ());
            hub(async {}).await;
            prepared();
            // Another task's calls aren't this one's.
            tokio::spawn(async { statement(|| ()) }).await.unwrap();
        })
        .await;
        assert_eq!((c.dbq, c.hubn, c.dbprep, c.dbconn), (2, 1, 1, 0));
        // Outside a request, nothing is counted and nothing fails.
        statement(|| ());
    }
}
