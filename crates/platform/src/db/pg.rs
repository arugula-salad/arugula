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
//! transaction mode (M144's `LISTEN` needs it too).

use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context as _, Result};
use bytes::BytesMut;
use tokio_postgres::{
    Client, NoTls, Statement,
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
    /// The key of the platform's write lock (`pg_advisory_xact_lock`):
    /// one per schema, so tests running side by side don't wait on each
    /// other.
    lock: i64,
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
    lock: i64,
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
        let lock = lock_key(schema.as_deref().unwrap_or("public"));
        let pool = Self {
            cfg,
            scratch: schema.is_some(),
            schema,
            lock,
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
        Ok(Session { client, lock: self.lock, statements: Default::default() })
    }
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

/// A stable key for the write lock of a schema (FNV-1a).
fn lock_key(schema: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in "arugula-platform:".bytes().chain(schema.bytes()) {
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

    /// Begin a transaction, holding the platform's write lock until it ends.
    pub fn begin(&self) -> Result<()> {
        let s = self.session().map_err(|e| anyhow::anyhow!("the platform's database: {e:#}"))?;
        s.batch(&format!("BEGIN; SELECT pg_advisory_xact_lock({})", s.lock))
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
            Value::Real(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                _ => f.to_sql(ty, out),
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
            Value::Blob(b) => b.as_slice().to_sql(ty, out),
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
