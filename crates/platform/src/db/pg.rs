//! Postgres for the platform (M143): a small pool of `tokio-postgres`
//! connections behind the same synchronous calls SQLite answers, so every
//! query in `db/` runs on either ([`super::sql`]).
//!
//! The connections are driven by a runtime of their own, on one thread
//! (`platform-pg`). A caller waits for its answer there
//! (`futures_executor::block_on`), from any thread, inside an async task
//! or not, as it waits for SQLite's lock today: a query is a round trip on
//! Fly's private network, well under a millisecond. Nothing holds a
//! connection across an `.await`, as nothing held SQLite's lock.
//!
//! Each connection keeps the statements it has prepared, so a query is
//! parsed once per connection. That needs Postgres itself or a pooler in
//! session mode: use the cluster's direct address, not PgBouncer's in
//! transaction mode ([`Pool::listen`]'s `LISTEN` needs it too).
//!
//! **Two machines (M144).** Nothing here serializes the platform's
//! transactions any more: each is Postgres's own `READ COMMITTED`, and the
//! queries that need it lock rows (`FOR UPDATE`, `SKIP LOCKED`) or take a
//! named lock ([`Session::lock`]) for an invariant rows can't hold.
//! [`Pool::listen`] is the one connection held for itself: it hears other
//! machines' notifications (`LISTEN`) and holds the leader's lock.

use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use bytes::BytesMut;
use tokio_postgres::{
    AsyncMessage, Client, NoTls, Statement,
    types::{FromSql, IsNull, ToSql, Type, to_sql_checked},
};

use super::sql::{Value, numbered};

/// How many connections the platform holds open.
const POOL: usize = 4;

/// The platform's connections to one database (or, for a test, one
/// schema of it).
pub struct Pool {
    cfg: tokio_postgres::Config,
    /// The schema its tables are in: Postgres's default (`public`), or a
    /// test's own, made if it isn't there.
    schema: Option<String>,
    /// What names its locks and its notifications' channel: the schema,
    /// so tests running side by side don't wait on or hear each other.
    scope: String,
    /// A test's schema, dropped with the pool.
    scratch: bool,
    slots: Vec<Mutex<Option<Session>>>,
    next: AtomicUsize,
    rt: tokio::runtime::Handle,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

/// One connection, and the statements it has prepared.
pub struct Session {
    client: Client,
    scope: String,
    statements: RefCell<HashMap<String, Statement>>,
}

/// A connection taken from the pool until it's dropped: the session, or
/// why there's none (the database is unreachable).
pub struct Lease<'a> {
    slot: MutexGuard<'a, Option<Session>>,
    failed: Option<anyhow::Error>,
}

impl Pool {
    /// The database at `url` (`postgres://...`), with its first connection
    /// made, so a wrong URL fails at start. `schema`: the tables are in
    /// that schema rather than `public` (tests).
    pub fn connect(url: &str, schema: Option<String>) -> Result<Self> {
        let mut cfg: tokio_postgres::Config = url.parse().context("the platform's database URL")?;
        cfg.application_name("arugula-platform");
        Self::connect_cfg(cfg, schema)
    }

    fn connect_cfg(cfg: tokio_postgres::Config, schema: Option<String>) -> Result<Self> {
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("platform-pg".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => return drop(tx.send(Err(e))),
            };
            let _ = tx.send(Ok(rt.handle().clone()));
            rt.block_on(async {
                let _ = stopped.await;
            });
        })?;
        let rt = rx.recv().context("the platform's database thread")??;
        let pool = Self {
            cfg,
            scratch: schema.is_some(),
            scope: schema.clone().unwrap_or_else(|| "public".into()),
            schema,
            slots: (0..POOL).map(|_| Mutex::new(None)).collect(),
            next: AtomicUsize::new(0),
            rt,
            stop: Some(stop),
        };
        let first = pool.lease();
        if let Some(e) = first.failed {
            return Err(e);
        }
        drop(first);
        Ok(pool)
    }

    /// Another pool on the same database and schema, which leaves the
    /// schema to this one to drop (tests: two machines on one database).
    #[cfg(any(test, feature = "contract"))]
    pub fn second(&self) -> Result<Self> {
        let mut pool = Self::connect_cfg(self.cfg.clone(), self.schema.clone())?;
        pool.scratch = false;
        Ok(pool)
    }

    /// The schema it's in, when it isn't Postgres's default.
    #[cfg(any(test, feature = "contract"))]
    pub fn schema(&self) -> Option<&str> {
        self.schema.as_deref()
    }

    /// A connection: a free one that's open, else a new one in a free
    /// place (so there are only as many as are used at once), else the next
    /// in turn once it's free. Made again if it was lost.
    pub fn lease(&self) -> Lease<'_> {
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        let n = self.slots.len();
        let free = |open: bool| {
            (0..n).find_map(|i| self.slots[(start + i) % n].try_lock().ok().filter(|s| s.is_some() == open))
        };
        let mut slot = free(true)
            .or_else(|| free(false))
            .unwrap_or_else(|| self.slots[start % n].lock().unwrap_or_else(|p| p.into_inner()));
        let mut failed = None;
        if slot.as_ref().is_none_or(|s| s.client.is_closed()) {
            *slot = None;
            match self.session() {
                Ok(s) => *slot = Some(s),
                Err(e) => failed = Some(e),
            }
        }
        Lease { slot, failed }
    }

    fn session(&self) -> Result<Session> {
        let cfg = self.cfg.clone();
        let made = self.rt.spawn(async move {
            let (client, connection) = cfg.connect(NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    tracing::warn!("the platform's database connection: {e}");
                }
            });
            Ok::<_, tokio_postgres::Error>(client)
        });
        let client = futures_executor::block_on(made)?.context("connecting to the platform's database")?;
        if let Some(s) = &self.schema {
            futures_executor::block_on(
                client.batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS \"{s}\"; SET search_path TO \"{s}\"")),
            )?;
        }
        Ok(Session { client, scope: self.scope.clone(), statements: Default::default() })
    }

    /// The connection held for itself (M144): it `LISTEN`s on the
    /// platform's channel ([`channel`]) and tries for the leader's lock
    /// (`pg_try_advisory_lock`, held for as long as this connection is
    /// open), and is made again whenever it's lost, then again tried for
    /// the lock every [`LEADER_TRY`]. Each notification, and each
    /// (re)connection, wakes [`Listening::heard`]: whoever waits on it reads
    /// what it may have missed. Returns once the first try is over, so a
    /// machine that can be the leader is one when this returns.
    ///
    /// Both ends probe the connection with TCP keepalives, so a leader
    /// whose machine vanished without closing it loses the lock within
    /// about a minute, and the other machine takes it; a leader cut off
    /// stops leading about as soon. The feed follower's writes are
    /// idempotent, so the moment both think they lead costs nothing.
    pub fn listen(&self) -> Listening {
        let listening = Listening { heard: Default::default(), leader: Default::default() };
        let (mut cfg, scope) = (self.cfg.clone(), self.scope.clone());
        // This side probes too, so a leader cut off from the database stops
        // thinking it leads about when the database lets the lock go.
        cfg.keepalives(true)
            .keepalives_idle(Duration::from_secs(20))
            .keepalives_interval(Duration::from_secs(10))
            .keepalives_retries(3);
        let (heard, leader) = (listening.heard.clone(), listening.leader.clone());
        let (first, tried) = std::sync::mpsc::channel::<()>();
        self.rt.spawn(async move {
            let mut first = Some(first);
            let mut wait = Duration::from_millis(250);
            loop {
                match listen_once(&cfg, &scope, &heard, &leader, &mut first).await {
                    Ok(()) => wait = Duration::from_millis(250),
                    Err(e) => tracing::warn!("the platform's listening connection: {e:#}"),
                }
                leader.store(false, Ordering::SeqCst);
                if let Some(f) = first.take() {
                    let _ = f.send(());
                }
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(10));
            }
        });
        let _ = tried.recv_timeout(Duration::from_secs(10));
        listening
    }
}

/// How often a machine that isn't the leader tries for the lock.
pub const LEADER_TRY: Duration = Duration::from_secs(5);

/// The channel the platform's machines notify each other on, for a
/// schema: `arugula_platform_public` in production.
pub fn channel(scope: &str) -> String {
    format!("arugula_platform_{scope}")
}

/// What [`Pool::listen`] gives: a wake-up for each notification (they
/// coalesce: one wake for several), and whether this machine holds the
/// leader's lock.
#[derive(Clone)]
pub struct Listening {
    pub heard: Arc<tokio::sync::Notify>,
    pub leader: Arc<AtomicBool>,
}

/// One listening connection, until it's lost.
async fn listen_once(
    cfg: &tokio_postgres::Config,
    scope: &str,
    heard: &tokio::sync::Notify,
    leader: &AtomicBool,
    first: &mut Option<std::sync::mpsc::Sender<()>>,
) -> Result<()> {
    use futures_util::StreamExt as _;
    let (client, mut connection) = cfg.connect(NoTls).await.context("connecting")?;
    let (lost_tx, mut lost) = tokio::sync::oneshot::channel::<()>();
    let wake = Arc::new(tokio::sync::Notify::new());
    let woke = wake.clone();
    let driver = tokio::spawn(async move {
        let mut messages = futures_util::stream::poll_fn(move |cx| connection.poll_message(cx));
        while let Some(m) = messages.next().await {
            match m {
                Ok(AsyncMessage::Notification(_)) => woke.notify_one(),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("the platform's listening connection: {e}");
                    break;
                }
            }
        }
        let _ = lost_tx.send(());
    });
    let setup = format!(
        "SET application_name = 'arugula-platform listen'; SET tcp_keepalives_idle = 20; \
         SET tcp_keepalives_interval = 10; SET tcp_keepalives_count = 3; LISTEN \"{}\"",
        channel(scope)
    );
    client.batch_execute(&setup).await.context("LISTEN")?;
    let key = lock_key(scope, "leader");
    // Read what was missed while this connection wasn't there.
    heard.notify_one();
    let r = async {
        loop {
            if !leader.load(Ordering::SeqCst) {
                let got: bool = client.query_one("SELECT pg_try_advisory_lock($1)", &[&key]).await?.get(0);
                if got {
                    tracing::info!("this machine leads the platform's feed follower and reports");
                }
                leader.store(got, Ordering::SeqCst);
            }
            if let Some(f) = first.take() {
                let _ = f.send(());
            }
            tokio::select! {
                _ = wake.notified() => heard.notify_one(),
                _ = tokio::time::sleep(LEADER_TRY) => {}
                _ = &mut lost => return Ok::<_, anyhow::Error>(()),
            }
        }
    }
    .await;
    driver.abort();
    r
}

impl Drop for Pool {
    fn drop(&mut self) {
        if self.scratch
            && let Some(s) = &self.schema
            && let Ok(session) = self.lease().session()
        {
            let _ = session.batch(&format!("DROP SCHEMA IF EXISTS \"{s}\" CASCADE"));
        }
        for slot in &self.slots {
            slot.lock().unwrap_or_else(|p| p.into_inner()).take();
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// A stable key for the lock `name` in a schema (FNV-1a).
fn lock_key(scope: &str, name: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in "arugula-platform:".bytes().chain(scope.bytes()).chain(*b":").chain(name.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h as i64
}

impl Lease<'_> {
    pub fn session(&self) -> Result<&Session, &anyhow::Error> {
        match (&*self.slot, &self.failed) {
            (Some(s), _) => Ok(s),
            (None, Some(e)) => Err(e),
            (None, None) => unreachable!("a lease has a session or why not"),
        }
    }

    /// Begin a transaction: Postgres's own `READ COMMITTED`, with no lock
    /// of the platform's (M144; [`Session::lock`] takes one).
    pub fn begin(&self) -> Result<()> {
        let s = self.session().map_err(|e| anyhow::anyhow!("the platform's database: {e:#}"))?;
        s.batch("BEGIN")
    }
}

impl Session {
    fn prepare(&self, sql: &str) -> Result<Statement> {
        if let Some(s) = self.statements.borrow().get(sql) {
            return Ok(s.clone());
        }
        let pg = numbered(sql);
        let s = futures_executor::block_on(self.client.prepare(&pg)).with_context(|| format!("preparing {pg}"))?;
        self.statements.borrow_mut().insert(sql.to_owned(), s.clone());
        Ok(s)
    }

    pub fn execute(&self, sql: &str, p: &[Value]) -> Result<usize> {
        let s = self.prepare(sql)?;
        let p: Vec<&(dyn ToSql + Sync)> = p.iter().map(|v| v as _).collect();
        Ok(futures_executor::block_on(self.client.execute(&s, &p))? as usize)
    }

    pub fn query(&self, sql: &str, p: &[Value]) -> Result<Vec<Vec<Value>>> {
        let s = self.prepare(sql)?;
        let p: Vec<&(dyn ToSql + Sync)> = p.iter().map(|v| v as _).collect();
        let rows = futures_executor::block_on(self.client.query(&s, &p))?;
        rows.iter().map(|r| (0..r.len()).map(|i| Ok(r.try_get::<_, Value>(i)?)).collect()).collect()
    }

    pub fn batch(&self, sql: &str) -> Result<()> {
        Ok(futures_executor::block_on(self.client.batch_execute(sql))?)
    }

    /// Hold the lock `name` until the transaction ends
    /// (`pg_advisory_xact_lock`): for an invariant no row holds, such as
    /// one migration runner at a time.
    pub fn lock(&self, name: &str) -> Result<()> {
        self.batch(&format!("SELECT pg_advisory_xact_lock({})", lock_key(&self.scope, name)))
    }

    /// Tell every machine's [`Pool::listen`] (`pg_notify`): at the
    /// transaction's commit, or at once outside one. The payload is short
    /// (`NOTIFY` takes under 8000 bytes): what to read, not what's in it.
    pub fn notify(&self, payload: &str) -> Result<()> {
        self.execute("SELECT pg_notify(?1, ?2)", &[Value::Text(channel(&self.scope)), Value::Text(payload.into())])?;
        Ok(())
    }
}

/// Whether `e` is a constraint the row broke (Postgres's class 23), as
/// SQLite's `ConstraintViolation`.
pub fn is_constraint(e: &anyhow::Error) -> bool {
    e.downcast_ref::<tokio_postgres::Error>()
        .and_then(tokio_postgres::Error::code)
        .is_some_and(|c| c.code().starts_with("23"))
}

type BoxError = Box<dyn Error + Sync + Send>;

/// A parameter takes the type Postgres decided for its place: an integer
/// goes into `BIGINT`, `INTEGER`, `SMALLINT` or `BOOLEAN` alike, and a
/// boolean into `BOOLEAN` or an integer, as SQLite takes either.
impl ToSql for Value {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxError> {
        match self {
            Value::Null => Ok(IsNull::Yes),
            Value::Int(n) => match *ty {
                Type::INT8 => n.to_sql(ty, out),
                Type::INT4 => i32::try_from(*n)?.to_sql(ty, out),
                Type::INT2 => i16::try_from(*n)?.to_sql(ty, out),
                Type::BOOL => (*n != 0).to_sql(ty, out),
                Type::FLOAT8 => (*n as f64).to_sql(ty, out),
                Type::TEXT | Type::VARCHAR => n.to_string().to_sql(ty, out),
                _ => Err(format!("an integer can't go where Postgres wants {ty}").into()),
            },
            Value::Bool(b) => match *ty {
                Type::BOOL => b.to_sql(ty, out),
                Type::INT8 => i64::from(*b).to_sql(ty, out),
                Type::INT4 => i32::from(*b).to_sql(ty, out),
                Type::INT2 => i16::from(*b).to_sql(ty, out),
                _ => Err(format!("a boolean can't go where Postgres wants {ty}").into()),
            },
            // Never an integer column's: Postgres would read the float's
            // bits as the integer (M144 found it copying SQLite's 2.5).
            Value::Real(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                Type::FLOAT8 => f.to_sql(ty, out),
                _ => Err(format!("a real number can't go where Postgres wants {ty}").into()),
            },
            Value::Text(s) => match *ty {
                Type::JSONB => {
                    out.extend_from_slice(&[1]);
                    out.extend_from_slice(s.as_bytes());
                    Ok(IsNull::No)
                }
                Type::BYTEA => s.as_bytes().to_sql(ty, out),
                _ => s.as_str().to_sql(ty, out),
            },
            Value::Blob(b) => match *ty {
                Type::BYTEA => b.as_slice().to_sql(ty, out),
                _ => Err(format!("bytes can't go where Postgres wants {ty}").into()),
            },
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

/// A column, whatever its type, as [`Value`]. A `NUMERIC` (Postgres's
/// `sum` of a `BIGINT`) isn't read: cast it (`CAST(sum(x) AS BIGINT)`).
impl<'a> FromSql<'a> for Value {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, BoxError> {
        Ok(match *ty {
            Type::BOOL => Value::Bool(bool::from_sql(ty, raw)?),
            Type::INT8 => Value::Int(i64::from_sql(ty, raw)?),
            Type::INT4 => Value::Int(i32::from_sql(ty, raw)?.into()),
            Type::INT2 => Value::Int(i16::from_sql(ty, raw)?.into()),
            Type::OID => Value::Int(u32::from_sql(ty, raw)?.into()),
            Type::FLOAT8 => Value::Real(f64::from_sql(ty, raw)?),
            Type::FLOAT4 => Value::Real(f32::from_sql(ty, raw)?.into()),
            Type::BYTEA => Value::Blob(raw.to_vec()),
            Type::JSONB => Value::Text(String::from_utf8(raw.get(1..).unwrap_or_default().to_vec())?),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::JSON => {
                Value::Text(String::from_sql(ty, raw)?)
            }
            _ => return Err(format!("the platform doesn't read Postgres's {ty}: cast it").into()),
        })
    }

    fn from_sql_null(_: &Type) -> Result<Self, BoxError> {
        Ok(Value::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}
