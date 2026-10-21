//! The one way the platform's queries run, on SQLite or on Postgres (M143).
//! A query is written once, in SQL both understand, with SQLite's `?N`
//! placeholders; on Postgres they become `$N` when it's prepared
//! ([`numbered`]), so a `?` is never Postgres's own operator (`jsonb ?`). Where the two must differ (JSON, chat's search, scalar
//! `max`), the query asks [`Sql::dialect`] and says each.
//!
//! - [`Db::c`](super::Db::c) is a connection ([`Conn`]): SQLite's one, or one
//!   of Postgres's pool. [`Conn::transaction`] and [`Conn::immediate`] are
//!   transactions ([`Tx`]), rolled back unless committed.
//! - A value goes in as a [`Param`] (`params![...]`) and comes out of a
//!   [`Row`] by [`FromValue`]. Both go through [`Value`], which converts
//!   integers and booleans to whatever the column is: Postgres's are
//!   `BIGINT` and `BOOLEAN`, SQLite's both `INTEGER`. In SQL, a boolean
//!   is written `TRUE`/`FALSE` (SQLite reads them as 1 and 0).
//! - A row that isn't there is an error from [`Sql::query_row`], made
//!   `None` by [`Optional::optional`], as with rusqlite.
//!
//! What both dialects take as written: `ON CONFLICT ... DO NOTHING/UPDATE`
//! (not `INSERT OR IGNORE/REPLACE`), `RETURNING` (not
//! `last_insert_rowid`), `IS DISTINCT FROM`, `octet_length`, and
//! `CAST(... AS BIGINT)` round a `sum` that Postgres would make `NUMERIC`.
//!
//! Transactions: on SQLite, `BEGIN` or `BEGIN IMMEDIATE`, one writer at a
//! time. On Postgres, `BEGIN`: `READ COMMITTED`, with no lock of the
//! platform's (M144), so two machines' transactions run side by side. A
//! transaction that reads and then writes on what it read locks the rows
//! it read ([`Sql::for_update`]); a claim skips rows another holds
//! (`FOR UPDATE ... SKIP LOCKED`, per dialect where it's written); and an
//! invariant no row can hold takes a named lock ([`Tx::lock`]).

use std::sync::MutexGuard;

use anyhow::{Context as _, Result, bail};

use super::{migrate::Dialect, pg};

/// A value in a row or a parameter, whichever database it's from.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

/// What a query takes as a parameter.
pub trait Param {
    fn value(&self) -> Result<Value>;
}

impl<T: Param + ?Sized> Param for &T {
    fn value(&self) -> Result<Value> {
        (**self).value()
    }
}

impl<T: Param> Param for Option<T> {
    fn value(&self) -> Result<Value> {
        match self {
            Some(v) => v.value(),
            None => Ok(Value::Null),
        }
    }
}

impl Param for Value {
    fn value(&self) -> Result<Value> {
        Ok(self.clone())
    }
}

impl Param for str {
    fn value(&self) -> Result<Value> {
        Ok(Value::Text(self.to_owned()))
    }
}

impl Param for String {
    fn value(&self) -> Result<Value> {
        Ok(Value::Text(self.clone()))
    }
}

impl Param for [u8] {
    fn value(&self) -> Result<Value> {
        Ok(Value::Blob(self.to_vec()))
    }
}

impl Param for Vec<u8> {
    fn value(&self) -> Result<Value> {
        Ok(Value::Blob(self.clone()))
    }
}

impl Param for bool {
    fn value(&self) -> Result<Value> {
        Ok(Value::Bool(*self))
    }
}

macro_rules! int_params {
    ($($t:ty),*) => {$(
        impl Param for $t {
            fn value(&self) -> Result<Value> {
                Ok(Value::Int(i64::try_from(*self).with_context(|| format!("{} is too big for the database", self))?))
            }
        }
    )*};
}
int_params!(i64, i32, u32, u64, usize);

/// A query's parameters, in order: `params![a, b]`.
macro_rules! params {
    () => { &[] as &[&dyn $crate::db::sql::Param] };
    ($($x:expr),+ $(,)?) => { &[$(&$x as &dyn $crate::db::sql::Param),+] as &[&dyn $crate::db::sql::Param] };
}
pub(crate) use params;

/// A list's items as parameters, for a query whose number of them varies
/// (`IN (?, ?, ...)`).
pub fn list<T: Param>(v: &[T]) -> Vec<&dyn Param> {
    v.iter().map(|x| x as &dyn Param).collect()
}

/// What a [`Row`]'s column becomes.
pub trait FromValue: Sized {
    fn from_value(v: Value) -> Result<Self>;
}

impl FromValue for Value {
    fn from_value(v: Value) -> Result<Self> {
        Ok(v)
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(v: Value) -> Result<Self> {
        match v {
            Value::Null => Ok(None),
            v => T::from_value(v).map(Some),
        }
    }
}

impl FromValue for String {
    fn from_value(v: Value) -> Result<Self> {
        match v {
            Value::Text(s) => Ok(s),
            v => bail!("expected text, found {v:?}"),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(v: Value) -> Result<Self> {
        match v {
            Value::Blob(b) => Ok(b),
            Value::Text(s) => Ok(s.into_bytes()),
            v => bail!("expected bytes, found {v:?}"),
        }
    }
}

impl FromValue for bool {
    fn from_value(v: Value) -> Result<Self> {
        match v {
            Value::Bool(b) => Ok(b),
            Value::Int(n) => Ok(n != 0),
            v => bail!("expected a boolean, found {v:?}"),
        }
    }
}

impl FromValue for f64 {
    fn from_value(v: Value) -> Result<Self> {
        match v {
            Value::Real(f) => Ok(f),
            Value::Int(n) => Ok(n as f64),
            v => bail!("expected a number, found {v:?}"),
        }
    }
}

macro_rules! int_values {
    ($($t:ty),*) => {$(
        impl FromValue for $t {
            fn from_value(v: Value) -> Result<Self> {
                match v {
                    Value::Int(n) => Ok(<$t>::try_from(n).with_context(|| format!("{n} doesn't fit {}", stringify!($t)))?),
                    Value::Bool(b) => Ok(<$t>::from(b)),
                    v => bail!("expected an integer, found {v:?}"),
                }
            }
        }
    )*};
}
int_values!(i64, i32, u32, u64, usize);

/// A row, read out of either database.
#[derive(Debug)]
pub struct Row(Vec<Value>);

impl Row {
    pub fn get<T: FromValue>(&self, i: usize) -> Result<T> {
        let v = self.0.get(i).cloned().with_context(|| format!("no column {i}"))?;
        T::from_value(v).with_context(|| format!("column {i}"))
    }
}

/// [`Sql::query_row`] found no row.
#[derive(Debug)]
pub struct NoRows;

impl std::fmt::Display for NoRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no row")
    }
}

impl std::error::Error for NoRows {}

/// A row that may not be there, as rusqlite's `OptionalExtension`.
pub trait Optional<T> {
    fn optional(self) -> Result<Option<T>>;
}

impl<T> Optional<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.is::<NoRows>() => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Where a statement runs.
#[doc(hidden)]
pub enum On<'a> {
    Sqlite(&'a rusqlite::Connection),
    Postgres(Result<&'a pg::Session, &'a anyhow::Error>),
}

/// A connection or a transaction, on either database.
pub trait Sql {
    #[doc(hidden)]
    fn on(&self) -> On<'_>;

    fn dialect(&self) -> Dialect {
        match self.on() {
            On::Sqlite(_) => Dialect::Sqlite,
            On::Postgres(_) => Dialect::Postgres,
        }
    }

    /// Run a statement; how many rows it changed.
    fn execute(&self, sql: &str, p: &[&dyn Param]) -> Result<usize> {
        let p = values(p)?;
        match self.on() {
            On::Sqlite(c) => Ok(c.prepare_cached(sql)?.execute(rusqlite::params_from_iter(p))?),
            On::Postgres(s) => session(s)?.execute(sql, &p),
        }
    }

    /// Every row a query answers, each made a `T` by `f`.
    fn query_map<T>(&self, sql: &str, p: &[&dyn Param], mut f: impl FnMut(&Row) -> Result<T>) -> Result<Vec<T>> {
        let p = values(p)?;
        match self.on() {
            On::Sqlite(c) => {
                let mut q = c.prepare_cached(sql)?;
                let n = q.column_count();
                let mut rows = q.query(rusqlite::params_from_iter(p))?;
                let mut out = Vec::new();
                while let Some(r) = rows.next()? {
                    let row = Row((0..n).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<_>>()?);
                    out.push(f(&row)?);
                }
                Ok(out)
            }
            On::Postgres(s) => session(s)?.query(sql, &p)?.into_iter().map(|r| f(&Row(r))).collect(),
        }
    }

    /// The first row a query answers, made a `T` by `f`; [`NoRows`] when
    /// there's none.
    fn query_row<T>(&self, sql: &str, p: &[&dyn Param], f: impl FnOnce(&Row) -> Result<T>) -> Result<T> {
        let (mut f, mut out) = (Some(f), None);
        self.query_map(sql, p, |r| {
            if let Some(f) = f.take() {
                out = Some(f(r)?);
            }
            Ok(())
        })?;
        out.ok_or_else(|| NoRows.into())
    }

    /// Statements without parameters, separated by semicolons.
    fn execute_batch(&self, sql: &str) -> Result<()> {
        match self.on() {
            On::Sqlite(c) => Ok(c.execute_batch(sql)?),
            On::Postgres(s) => session(s)?.batch(sql),
        }
    }

    /// What a `SELECT` in a transaction ends with to lock the rows it
    /// reads until the transaction ends: ` FOR UPDATE` on Postgres, nothing
    /// on SQLite, whose `BEGIN IMMEDIATE` already holds every row (M144).
    fn for_update(&self) -> &'static str {
        match self.dialect() {
            Dialect::Sqlite => "",
            Dialect::Postgres => " FOR UPDATE",
        }
    }

    /// Wake the platform's other machines (`pg_notify`, at commit inside a
    /// transaction): `payload` says what to read again, never what's in it.
    /// Nothing on SQLite, which is one machine.
    fn notify(&self, payload: &str) -> Result<()> {
        match self.on() {
            On::Sqlite(_) => Ok(()),
            On::Postgres(s) => session(s)?.notify(payload),
        }
    }
}

fn values(p: &[&dyn Param]) -> Result<Vec<Value>> {
    p.iter().map(|v| v.value()).collect()
}

fn session<'a>(s: Result<&'a pg::Session, &anyhow::Error>) -> Result<&'a pg::Session> {
    s.map_err(|e| anyhow::anyhow!("the platform's database: {e:#}"))
}

impl rusqlite::ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        use rusqlite::types::{ToSqlOutput, ValueRef};
        Ok(ToSqlOutput::Borrowed(match self {
            Value::Null => ValueRef::Null,
            Value::Int(n) => ValueRef::Integer(*n),
            Value::Real(f) => ValueRef::Real(*f),
            Value::Text(s) => ValueRef::Text(s.as_bytes()),
            Value::Blob(b) => ValueRef::Blob(b),
            Value::Bool(b) => ValueRef::Integer(i64::from(*b)),
        }))
    }
}

impl rusqlite::types::FromSql for Value {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        use rusqlite::types::ValueRef;
        Ok(match v {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::Int(n),
            ValueRef::Real(f) => Value::Real(f),
            ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Value::Blob(b.to_vec()),
        })
    }
}

/// SQLite's connection itself (migrations and their tests).
impl Sql for rusqlite::Connection {
    fn on(&self) -> On<'_> {
        On::Sqlite(self)
    }
}

/// A connection: SQLite's, or one of Postgres's pool, held until dropped.
pub struct Conn<'a>(pub(super) Inner<'a>);

pub(super) enum Inner<'a> {
    Sqlite(MutexGuard<'a, rusqlite::Connection>),
    Postgres(pg::Lease<'a>),
}

impl Sql for Conn<'_> {
    fn on(&self) -> On<'_> {
        match &self.0 {
            Inner::Sqlite(c) => On::Sqlite(c),
            Inner::Postgres(l) => On::Postgres(l.session()),
        }
    }
}

impl<'a> Conn<'a> {
    /// A transaction. On SQLite it takes the write lock at its first write
    /// (`BEGIN`); on Postgres it takes none (see the module's docs).
    pub fn transaction(&mut self) -> Result<Tx<'_, 'a>> {
        self.begin("BEGIN")
    }

    /// A transaction that takes the write lock at once (SQLite's `BEGIN
    /// IMMEDIATE`): what reads and then writes on what it read. On Postgres
    /// it's [`transaction`](Self::transaction): such a transaction locks
    /// the rows it reads ([`Sql::for_update`]).
    pub fn immediate(&mut self) -> Result<Tx<'_, 'a>> {
        self.begin("BEGIN IMMEDIATE")
    }

    fn begin(&mut self, sqlite: &str) -> Result<Tx<'_, 'a>> {
        match &self.0 {
            Inner::Sqlite(c) => c.execute_batch(sqlite)?,
            Inner::Postgres(l) => l.begin()?,
        }
        Ok(Tx { conn: self, done: false })
    }
}

/// A transaction, rolled back when dropped uncommitted.
pub struct Tx<'t, 'a> {
    conn: &'t mut Conn<'a>,
    done: bool,
}

impl Tx<'_, '_> {
    /// Hold the lock `name` until this transaction ends, on Postgres
    /// (`pg_advisory_xact_lock`), where machines' transactions otherwise
    /// run side by side; nothing on SQLite, whose writers already take
    /// turns. Only for an invariant no row holds (M144's lock inventory in
    /// docs/control-ops.md).
    pub fn lock(&self, name: &str) -> Result<()> {
        match self.on() {
            On::Sqlite(_) => Ok(()),
            On::Postgres(s) => session(s)?.lock(name),
        }
    }

    pub fn commit(mut self) -> Result<()> {
        self.execute_batch("COMMIT")?;
        self.done = true;
        Ok(())
    }
}

impl Drop for Tx<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.execute_batch("ROLLBACK");
        }
    }
}

impl Sql for Tx<'_, '_> {
    fn on(&self) -> On<'_> {
        self.conn.on()
    }
}

/// `sql` with SQLite's placeholders as Postgres's: `?N` becomes `$N`, and
/// a bare `?` the next number after the highest so far (as SQLite numbers
/// it). Quoted text and comments are left alone.
pub fn numbered(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut highest = 0u32;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => {
                out.push(c);
                for d in chars.by_ref() {
                    out.push(d);
                    if d == c {
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                out.push(c);
                for d in chars.by_ref() {
                    out.push(d);
                    if d == '\n' {
                        break;
                    }
                }
            }
            '?' => {
                let mut digits = String::new();
                while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                    digits.push(d);
                    chars.next();
                }
                let n = if digits.is_empty() { highest + 1 } else { digits.parse().unwrap_or(0) };
                highest = highest.max(n);
                out.push('$');
                out.push_str(&n.to_string());
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_numbered_as_sqlite_numbers_them() {
        assert_eq!(numbered("a = ?1 AND b = ?2 OR c = ?1"), "a = $1 AND b = $2 OR c = $1");
        assert_eq!(numbered("x IN (?, ?, ?) AND y = ?"), "x IN ($1, $2, $3) AND y = $4");
        assert_eq!(numbered("x = ?3 AND y = ?"), "x = $3 AND y = $4");
        assert_eq!(
            numbered("x = '?1' AND \"?\" = ?1 -- ?2\n AND z = ?2"),
            "x = '?1' AND \"?\" = $1 -- ?2\n AND z = $2"
        );
    }

    #[test]
    fn values_convert_as_columns_need() {
        assert!(bool::from_value(Value::Int(1)).unwrap());
        assert_eq!(u64::from_value(Value::Bool(true)).unwrap(), 1);
        assert!(u64::from_value(Value::Int(-1)).is_err());
        assert_eq!(Option::<String>::from_value(Value::Null).unwrap(), None);
        assert!(u64::MAX.value().is_err());
        assert!(Err::<(), _>(anyhow::Error::from(NoRows)).optional().unwrap().is_none());
    }
}
