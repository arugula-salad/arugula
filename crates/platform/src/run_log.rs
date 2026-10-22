//! A run's durable log (platform step 7): what the CLI's `arugula runs
//! logs` and MCP's `read_run` read (M216).
//!
//! M213 keeps it: a hosted machine uploads its run's log to the run-log
//! bucket in chunks (`crate::run_logs`, hosted workspaces only, kept 30
//! days), and [`page`] reads a page of it there, each chunk checked against
//! its size and the SHA-256 its machine sent. A run with no chunk kept (a
//! person's machine's, one from before M213, one past its days, or a
//! platform with no bucket) has none: its output is in its pane on its
//! machine, which its `running` event names, and the project's people watch
//! end to end.
//!
//! The routes (`GET /api/runs/{run}/log` for a person,
//! `GET /api/daemon/runs/{run}/log` for a machine's agents, M216) have
//! checked who may read the run before they ask here.

use anyhow::{Result, bail};
use bytes::Bytes;
use sha2::{Digest as _, Sha256};

use crate::{
    Platform,
    db::{run_logs::LogChunk, runs::Run},
    run_logs::Bucket,
};

/// Where a page starts: a byte offset, or so that it ends at the end of
/// the log as it is now (its tail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    Offset(u64),
    Tail,
}

/// A page of a run's kept log: its text (lossy UTF-8 of the bytes read),
/// and whether it reaches the end of the log as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    /// The offset of the first byte read.
    pub offset: u64,
    /// The offset after the last byte read.
    pub next_offset: u64,
    /// No byte after `next_offset` is kept (yet).
    pub at_end: bool,
}

/// At most `max` bytes of `run`'s kept log from `from`, read from the
/// run-log bucket. `None`: the platform keeps no log for it. Bytes the
/// machine's pane dropped before it sent them (a gap between two chunks),
/// and a chunk the bucket's rule already deleted, are skipped: the page
/// goes on from the next chunk. Each chunk is read from the bucket once
/// while it's in [`crate::run_logs::bucket::ChunkCache`], so a log followed
/// a page every few seconds doesn't read it again each time.
pub async fn page(app: &Platform, run: &Run, from: From, max: u64) -> Result<Option<Page>> {
    let Some(b) = &app.run_logs else { return Ok(None) };
    let now = arugula_e2e::now_ms();
    let chunks = app.db.run_log_chunks(&run.id, now.saturating_sub(b.ttl_ms))?;
    let Some(last) = chunks.last() else { return Ok(None) };
    let end = last.end();
    let offset = match from {
        From::Offset(o) => o,
        From::Tail => end.saturating_sub(max),
    };
    let mut out = Vec::new();
    let mut first = None;
    let mut at = offset;
    for c in chunks.iter().filter(|c| c.end() > offset) {
        if out.len() as u64 >= max {
            break;
        }
        let Some(bytes) = chunk(app, b, c, now).await? else { continue };
        let from = at.max(c.offset);
        let want = (max - out.len() as u64).min(c.end() - from);
        let start = (from - c.offset) as usize;
        out.extend_from_slice(&bytes[start..start + want as usize]);
        first.get_or_insert(from);
        at = from + want;
    }
    let next_offset = at.max(offset.min(end));
    Ok(Some(Page {
        text: String::from_utf8_lossy(&out).into_owned(),
        offset: first.unwrap_or(next_offset),
        next_offset,
        at_end: next_offset >= end,
    }))
}

/// Chunk `c`'s bytes, checked: from the cache, else from the bucket.
/// `None`: the bucket has none there any more (its rule took it).
async fn chunk(app: &Platform, b: &Bucket, c: &LogChunk, now: u64) -> Result<Option<Bytes>> {
    if let Some(bytes) = b.cache.get(&c.bucket_key) {
        return Ok(Some(bytes));
    }
    let url = b.presign("GET", &c.bucket_key, None, 60, now);
    // Never the URL in an error: it's signed.
    let r = app.http.get(url).send().await.map_err(|e| e.without_url())?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !r.status().is_success() {
        bail!("the run-log bucket answered {} for {}", r.status(), c.bucket_key);
    }
    let bytes = r.bytes().await.map_err(|e| e.without_url())?;
    if bytes.len() as u64 != c.size {
        bail!("{} in the run-log bucket is {} bytes, not the {} its machine sent", c.bucket_key, bytes.len(), c.size);
    }
    if hex::encode(Sha256::digest(&bytes)) != c.sha256 {
        bail!("{} in the run-log bucket isn't what its machine sent", c.bucket_key);
    }
    b.cache.put(&c.bucket_key, bytes.clone());
    Ok(Some(bytes))
}
