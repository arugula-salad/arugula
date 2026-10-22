//! Run logs (M213): where a hosted run's log is in the run-log bucket, a
//! chunk a row (`run_log_chunks`, made by `0026_run_logs`), beside its
//! artifacts (`run_artifacts`, M210). What may be sent, and who reads it,
//! is `crate::run_logs`'s; this is storage, the sweep that forgets what's
//! past the run-log days, and the operator's usage.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
    taken,
};

/// One chunk of a run's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogChunk {
    pub run: String,
    pub n: u32,
    /// Where it starts in the pane's output.
    pub offset: u64,
    pub size: u64,
    /// Its SHA-256, hex, as its machine said.
    pub sha256: String,
    pub bucket_key: String,
    pub created_at: u64,
}

impl LogChunk {
    /// Just past its last byte.
    pub fn end(&self) -> u64 {
        self.offset + self.size
    }
}

/// An upload URL asked for ([`Db::issue_run_upload`]), and the caps it's
/// counted against.
#[derive(Debug, Clone)]
pub struct Issue<'a> {
    pub run: &'a str,
    pub project: &'a str,
    pub key: &'a str,
    /// `log` or `artifact`.
    pub kind: &'a str,
    pub size: u64,
    pub now: u64,
    pub expires_at: u64,
    /// The most URLs a run is given in a minute.
    pub per_minute: u64,
    /// The most bytes a project's runs are given URLs for in a day.
    pub daily_bytes: u64,
    /// The most artifacts a run keeps.
    pub max_artifacts: u64,
}

/// One project's run logs and artifacts in the bucket, as the platform's
/// rows say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub project: String,
    pub runs: u64,
    pub log_bytes: u64,
    pub artifact_bytes: u64,
}

const COLS: &str = "run_id, n, byte_offset, size, sha256, bucket_key, created_at";

fn chunk_row(r: &Row) -> Result<LogChunk> {
    Ok(LogChunk {
        run: r.get(0)?,
        n: r.get::<u64>(1)? as u32,
        offset: r.get(2)?,
        size: r.get(3)?,
        sha256: r.get(4)?,
        bucket_key: r.get(5)?,
        created_at: r.get(6)?,
    })
}

/// Every project's run logs and artifacts, most bytes first (the
/// operator's `run-logs`).
pub fn usage(c: &impl Sql) -> Result<Vec<Usage>> {
    let mut out: std::collections::BTreeMap<String, Usage> = Default::default();
    for (table, logs) in [("run_log_chunks", true), ("run_artifacts", false)] {
        let rows = c.query_map(
            &format!(
                "SELECT r.project_id, count(DISTINCT r.id), CAST(COALESCE(SUM(t.size), 0) AS BIGINT)
                 FROM {table} t JOIN runs r ON r.id = t.run_id GROUP BY r.project_id"
            ),
            params![],
            |r| Ok((r.get::<String>(0)?, r.get::<u64>(1)?, r.get::<u64>(2)?)),
        )?;
        for (project, runs, bytes) in rows {
            let u = out.entry(project.clone()).or_insert_with(|| Usage { project, ..Default::default() });
            u.runs = u.runs.max(runs);
            if logs {
                u.log_bytes = bytes;
            } else {
                u.artifact_bytes = bytes;
            }
        }
    }
    let mut out: Vec<Usage> = out.into_values().collect();
    out.sort_by_key(|u| std::cmp::Reverse(u.log_bytes + u.artifact_bytes));
    Ok(out)
}

impl Db {
    /// Run `run`'s log chunks kept since `since`, in order.
    pub fn run_log_chunks(&self, run: &str, since: u64) -> Result<Vec<LogChunk>> {
        self.c().query_map(
            &format!("SELECT {COLS} FROM run_log_chunks WHERE run_id = ?1 AND created_at >= ?2 ORDER BY n"),
            params![run, since],
            chunk_row,
        )
    }

    /// Record `c`, if it's the run's next chunk: `false` when it isn't (the
    /// run has another count of chunks). One that's there is
    /// [`super::Taken`].
    pub fn add_run_log_chunk(&self, c: &LogChunk) -> Result<bool> {
        let made = self
            .c()
            .execute(
                &format!(
                    "INSERT INTO run_log_chunks ({COLS})
                     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 FROM runs
                     WHERE id = ?1 AND (SELECT count(*) FROM run_log_chunks WHERE run_id = ?1) = ?2"
                ),
                params![c.run, c.n, c.offset, c.size, c.sha256, c.bucket_key, c.created_at],
            )
            .map_err(taken)?;
        Ok(made == 1)
    }

    /// Forget every log chunk and artifact recorded before `before`: the
    /// bucket's rule takes their files. Their keys, to delete from the
    /// bucket too.
    pub fn sweep_run_logs(&self, before: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let mut keys = Vec::new();
        for table in ["run_log_chunks", "run_artifacts"] {
            keys.extend(tx.query_map(
                &format!("DELETE FROM {table} WHERE created_at < ?1 RETURNING bucket_key"),
                params![before],
                |r| r.get::<String>(0),
            )?);
        }
        tx.commit()?;
        Ok(keys)
    }

    /// Sign out an upload URL for `u`, if the run's caps and its project's
    /// daily bytes have room, counting the URLs still out: in one
    /// transaction that holds the run's row, so two asks can't both take the
    /// last of it. The refusal's words otherwise.
    pub fn issue_run_upload(&self, u: &Issue) -> Result<Result<(), String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held = tx
            .query_row(&format!("SELECT id FROM runs WHERE id = ?1{}", tx.for_update()), params![u.run], |r| {
                r.get::<String>(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(Err("no such run".into()));
        }
        let count = |sql: &str, p: &[&dyn super::sql::Param]| tx.query_row(sql, p, |r| r.get::<u64>(0));
        let lately = count(
            "SELECT count(*) FROM run_upload_urls WHERE run_id = ?1 AND issued_at > ?2",
            params![u.run, u.now.saturating_sub(60_000)],
        )?;
        if lately >= u.per_minute {
            return Ok(Err(format!("the run asked for {lately} uploads in a minute: try again shortly")));
        }
        let today = count(
            "SELECT CAST(COALESCE(SUM(size), 0) AS BIGINT) FROM run_upload_urls WHERE project_id = ?1 AND issued_at > ?2",
            params![u.project, u.now.saturating_sub(86_400_000)],
        )?;
        if today + u.size > u.daily_bytes {
            return Ok(Err(format!(
                "the project's runs sent their most for a day ({} bytes): the rest stays in the pane",
                u.daily_bytes
            )));
        }
        if u.kind == "artifact" {
            // Kept, and out (not expired, not kept yet), but this key.
            let n = count("SELECT count(*) FROM run_artifacts WHERE run_id = ?1", params![u.run])?
                + count(
                    "SELECT count(DISTINCT bucket_key) FROM run_upload_urls
                 WHERE run_id = ?1 AND kind = 'artifact' AND expires_at > ?2 AND bucket_key <> ?3
                   AND bucket_key NOT IN (SELECT bucket_key FROM run_artifacts WHERE run_id = ?1)",
                    params![u.run, u.now, u.key],
                )?;
            if n >= u.max_artifacts {
                return Ok(Err(format!("the run has the most artifacts it keeps ({})", u.max_artifacts)));
            }
        }
        tx.execute(
            "INSERT INTO run_upload_urls (id, run_id, project_id, bucket_key, kind, size, issued_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                hex::encode(arugula_e2e::random::<16>()),
                u.run,
                u.project,
                u.key,
                u.kind,
                u.size,
                u.now,
                u.expires_at
            ],
        )?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Record a run's artifact, if the run has fewer than `max`: counted in
    /// the transaction that adds it, holding the run's row. `None`: there's
    /// no such run, or it has `max`; a name it has is [`super::Taken`].
    pub fn add_run_artifact_capped(
        &self,
        a: &super::runs::Artifact,
        max: u64,
    ) -> Result<Option<super::runs::Artifact>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held = tx
            .query_row(&format!("SELECT id FROM runs WHERE id = ?1{}", tx.for_update()), params![a.run], |r| {
                r.get::<String>(0)
            })
            .optional()?;
        let n: u64 =
            tx.query_row("SELECT count(*) FROM run_artifacts WHERE run_id = ?1", params![a.run], |r| r.get(0))?;
        if held.is_none() || n >= max {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO run_artifacts (run_id, name, size, digest, bucket_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![a.run, a.name, a.size, a.digest, a.bucket_key, a.created_at],
        )
        .map_err(taken)?;
        tx.commit()?;
        Ok(Some(a.clone()))
    }

    /// The upload URLs that expired more than `margin` before `now` with
    /// nothing recorded at their key and no URL for it still out: their
    /// rows go, and their keys, to delete from the bucket (whatever was put
    /// there unsaid). Rows of URLs a day old that did get recorded go too.
    pub fn sweep_run_uploads(&self, now: u64, margin: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let keys: Vec<String> = tx.query_map(
            "SELECT DISTINCT u.bucket_key FROM run_upload_urls u
             WHERE u.expires_at < ?1
               AND u.bucket_key NOT IN (SELECT bucket_key FROM run_log_chunks)
               AND u.bucket_key NOT IN (SELECT bucket_key FROM run_artifacts)
               AND NOT EXISTS (SELECT 1 FROM run_upload_urls o WHERE o.bucket_key = u.bucket_key AND o.expires_at >= ?1)",
            params![now.saturating_sub(margin)],
            |r| r.get(0),
        )?;
        for k in &keys {
            tx.execute("DELETE FROM run_upload_urls WHERE bucket_key = ?1", params![k])?;
        }
        tx.execute(
            "DELETE FROM run_upload_urls WHERE expires_at < ?1 AND issued_at < ?2",
            params![now.saturating_sub(margin), now.saturating_sub(86_400_000)],
        )?;
        tx.commit()?;
        Ok(keys)
    }

    /// [`usage`], on this database.
    pub fn run_log_usage(&self) -> Result<Vec<Usage>> {
        usage(&self.c())
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction, after the projects went and before
/// [`super::runs::erase_account`] takes their runs: the chunks and upload
/// URLs of those runs. Their keys and their artifacts', to delete from the
/// bucket once the transaction is in.
pub(crate) fn erase_account(c: &impl Sql) -> Result<Vec<String>> {
    let gone = "SELECT id FROM runs WHERE project_id NOT IN (SELECT id FROM projects)";
    let mut keys = Vec::new();
    for table in ["run_log_chunks", "run_upload_urls"] {
        keys.extend(c.query_map(
            &format!("DELETE FROM {table} WHERE run_id IN ({gone}) RETURNING bucket_key"),
            params![],
            |r| r.get::<String>(0),
        )?);
    }
    keys.extend(c.query_map(
        &format!("SELECT bucket_key FROM run_artifacts WHERE run_id IN ({gone})"),
        params![],
        |r| r.get::<String>(0),
    )?);
    keys.sort();
    keys.dedup();
    Ok(keys)
}
