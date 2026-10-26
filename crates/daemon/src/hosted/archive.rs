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
//!
//! Before the platform rebuilds the box, which takes its open blocks with
//! it unclosed, it asks for them all at once ([`flush`], `hosted.archive`,
//! Arugula #209), so a session moved to the new box starts from its
//! conversation.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use arugula_control_wire::{
    forge::OpAnswer,
    transcripts::{HOSTED_TRANSCRIPTS, MAX_TEXT, TranscriptPost, fit},
};
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

/// This daemon's control, when it's a hosted machine.
fn hosted() -> Option<Arc<Control>> {
    let control = CONTROL.get().cloned()?;
    control.enrolled().is_some_and(|e| e.saved.hosted.is_some()).then_some(control)
}

/// An agent block closed: archive it, if this is a hosted machine.
pub fn closed(rt: &tokio::runtime::Handle, c: Closed) {
    let Some(control) = hosted() else { return };
    let p = post(c, arugula_e2e::now_ms());
    rt.spawn(async move {
        send(&control, &p, 4).await;
    });
}

/// How long `hosted.archive` takes at most: under the platform's wait for
/// its answer (10 s), so it hears one.
const FLUSH_WITHIN: Duration = Duration::from_secs(8);

/// `hosted.archive` (Arugula #209): the platform is about to rebuild this
/// box, so every open agent block's transcript goes now, as closing it
/// would send it. The blocks stay open; the box goes. Answered once the
/// sends are done, or past [`FLUSH_WITHIN`]. A transcript the platform
/// already has for a block stays as it was.
pub async fn flush() -> OpAnswer {
    let Some(control) = hosted() else {
        return OpAnswer::Cant { why: "not_hosted: this machine isn't a hosted workspace's box".into() };
    };
    let now = arugula_e2e::now_ms();
    let posts: Vec<TranscriptPost> = crate::agent::open_transcripts().into_iter().map(|c| post(c, now)).collect();
    let n = posts.len();
    let sends = futures_util::future::join_all(posts.iter().map(|p| send(&control, p, 2)));
    match tokio::time::timeout(FLUSH_WITHIN, sends).await {
        Ok(sent) => {
            let kept = sent.iter().filter(|k| **k).count();
            info!(open = n, kept, "hosted: archived the open agents' transcripts before a rebuild");
            if kept == n {
                OpAnswer::Done { url: None }
            } else {
                OpAnswer::Failed { error: format!("{} of {n} transcripts weren't archived", n - kept) }
            }
        }
        Err(_) => {
            warn!(open = n, "hosted: archiving the open agents' transcripts ran out of time");
            OpAnswer::Failed { error: "archiving the transcripts ran out of time".into() }
        }
    }
}

/// Send `p` to control, trying `attempts` times: whether it was kept.
async fn send(control: &Control, p: &TranscriptPost, attempts: u32) -> bool {
    let body = match serde_json::to_value(p) {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "hosted: can't send a transcript");
            return false;
        }
    };
    for attempt in 0..attempts {
        match control.cloud_now(reqwest::Method::POST, HOSTED_TRANSCRIPTS, Some(&body)).await {
            Ok(v) => {
                info!(block = p.block, transcript = v["id"].as_str().unwrap_or("?"), "hosted: archived a transcript");
                return true;
            }
            Err(crate::control::CloudRefused::Said { status, said, .. }) if status < 500 => {
                warn!(block = p.block, status, said, "hosted: control refused a transcript");
                return false;
            }
            Err(e) => {
                warn!(block = p.block, attempt, error = %e, "hosted: couldn't send a transcript");
                if attempt + 1 < attempts {
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt + 1))).await;
                }
            }
        }
    }
    false
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
