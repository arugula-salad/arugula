//! A hosted machine's closed agent blocks (M184, #1245). While an agent
//! block is open its transcript is on the box's disk, in the block's log,
//! like any machine's. When someone closes it, this sends its text to
//! control (`/api/hosted/transcripts`, signed as the hosted machine), and
//! the platform keeps it read only under the workspace's project, for the
//! project's people to read once the box is gone.
//!
//! Only on a hosted machine: [`start`] turns it on, and a daemon that isn't
//! enrolled as one sends nothing. The text is cut to fit
//! ([`arugula_control_wire::transcripts::fit`]: the start and the end
//! kept). A send that fails is tried a few times, then left: the block's
//! log stays on the box until the box goes.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use arugula_control_wire::transcripts::{HOSTED_TRANSCRIPTS, MAX_TEXT, TranscriptPost, fit};
use tracing::{info, warn};

use crate::control::Control;

static CONTROL: OnceLock<Arc<Control>> = OnceLock::new();

/// Turn archiving on: this daemon runs `--hosted`.
pub fn start(control: Arc<Control>) {
    let _ = CONTROL.set(control);
}

/// What an agent block leaves as it closes.
pub struct Closed {
    pub block: u64,
    /// `claude`, `codex` or `acp`.
    pub agent: String,
    pub title: String,
    pub cwd: Option<String>,
    pub started_at: u64,
    /// Its text (`capture --text`).
    pub text: String,
}

/// The most a signed body may be at the hub, less room for the rest.
const MAX_BODY: usize = (1 << 20) - 8 * 1024;

/// The post for `c`: its text cut until the whole body fits under the
/// hub's limit (JSON's escapes can make it longer than the text).
fn post(c: Closed, closed_at: u64) -> TranscriptPost {
    let mut max = MAX_TEXT;
    loop {
        let (text, cut) = fit(&c.text, max);
        let p = TranscriptPost {
            block: c.block,
            agent: c.agent.clone(),
            title: c.title.clone(),
            cwd: c.cwd.clone(),
            started_at: c.started_at,
            closed_at,
            cut,
            text,
        };
        let size = serde_json::to_vec(&p).map_or(usize::MAX, |b| b.len());
        if size <= MAX_BODY || max < 1024 {
            return p;
        }
        max /= 2;
    }
}

/// An agent block closed: archive it, if this is a hosted machine.
pub fn closed(rt: &tokio::runtime::Handle, c: Closed) {
    let Some(control) = CONTROL.get().cloned() else { return };
    if control.enrolled().is_none_or(|e| e.saved.hosted.is_none()) {
        return;
    }
    let p = post(c, arugula_e2e::now_ms());
    rt.spawn(async move {
        let body = match serde_json::to_value(&p) {
            Ok(b) => b,
            Err(e) => return warn!(error = %e, "hosted: can't send a transcript"),
        };
        for attempt in 0..4u32 {
            match control.cloud_now(reqwest::Method::POST, HOSTED_TRANSCRIPTS, Some(&body)).await {
                Ok(v) => {
                    return info!(
                        block = p.block,
                        transcript = v["id"].as_str().unwrap_or("?"),
                        "hosted: archived a transcript"
                    );
                }
                Err(crate::control::CloudRefused::Said { status, said, .. }) if status < 500 => {
                    return warn!(block = p.block, status, said, "hosted: control refused a transcript");
                }
                Err(e) => {
                    warn!(block = p.block, attempt, error = %e, "hosted: couldn't send a transcript");
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt + 1))).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_fits_the_hubs_limit_even_with_escapes() {
        let c = |text: String| Closed {
            block: 4,
            agent: "claude".into(),
            title: "t".into(),
            cwd: None,
            started_at: 1,
            text,
        };
        let p = post(c("> hi".into()), 9);
        assert_eq!((p.text.as_str(), p.cut, p.closed_at), ("> hi", false, 9));
        // Quotes double in JSON: cut further until the body fits.
        let p = post(c("\"".repeat(MAX_TEXT)), 9);
        assert!(p.cut && serde_json::to_vec(&p).unwrap().len() <= MAX_BODY);
    }
}
