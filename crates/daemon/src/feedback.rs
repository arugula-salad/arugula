//! Feedback (Arugula #1216): what a person or their agent writes, sent to
//! the hosted control's `FEEDBACK` (`arugula_control_wire::feedback`) for
//! the `feedback.send` operation (`ops/feedback.rs`), which fills in the
//! context first.
//!
//! - **Signed** when the machine is joined to the control it goes to, as
//!   telemetry's reports are (`telemetry::Signer`); a signature control
//!   refuses (401) goes again unsigned. **Unsigned**, it carries an install
//!   id (telemetry's while that's on, else one of its own in
//!   [`INSTALL_FILE`]) and a `FeedbackStamp` solved off the runtime. Control
//!   asking a harder stamp (`POW_BAD`) is solved for once more.
//! - **Queued** where control can't be reached (no answer, a 5xx, or no
//!   such route yet): kept in [`QUEUE_FILE`] and sent again with backoff,
//!   by the daemon's loop ([`start`]). What's queued is the send as written,
//!   with nobody's signature or stamp: those are made again when it goes.
//!   A week unsent, it's dropped.
//! - **Refused** for good (too many, anonymous sends off, too large):
//!   the caller gets a sentence of ours, never control's code.
//!
//! It goes where telemetry's reports go: the hosted control, or
//! `ARUGULA_TELEMETRY_URL`. Telemetry's consent has no say in any of this.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use arugula_control_wire::feedback::{
    ANONYMOUS_OFF, FEEDBACK, FeedbackAck, FeedbackRefused, FeedbackStamp, FeedbackSubmit, MAX_BODY, MAX_POW_DIFFICULTY,
    MAX_TITLE, POW_BAD, POW_DIFFICULTY, POW_MISSING, RATE_LIMITED, TOO_LARGE,
};
use arugula_e2e::now_ms;
use arugula_proto::telemetry as consent;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::telemetry::Signer;

/// The MCP tool's name.
pub const TOOL: &str = "feedback";
/// The sends waiting for control, in the state dir.
pub const QUEUE_FILE: &str = "feedback.queue.json";
/// The install id an unsigned send carries while telemetry has none.
pub const INSTALL_FILE: &str = "feedback.install";
/// The most sends kept waiting: past it, the oldest goes.
const MAX_QUEUED: usize = 50;
/// How long a send waits for control before it's dropped.
const KEEP_MS: u64 = 7 * 24 * 3600 * 1000;

/// What came of a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Taken(FeedbackAck),
    /// Control couldn't be reached: it waits in [`QUEUE_FILE`].
    Queued,
    /// Refused, with the status to answer and the sentence to say.
    Refused(StatusCode, String),
}

/// When the loop tries again: `first` after a try that failed, doubling to
/// `most`.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub first: Duration,
    pub most: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff { first: Duration::from_secs(60), most: Duration::from_secs(3600) }
    }
}

/// The daemon's feedback sender.
pub struct Feedback {
    dir: PathBuf,
    url: String,
    http: reqwest::Client,
    signer: Arc<dyn Signer>,
    backoff: Backoff,
    /// The difficulty an unsigned send is solved for first.
    difficulty: u8,
    /// The queue's file, one writer at a time.
    queue: Mutex<()>,
    wake: tokio::sync::Notify,
}

/// One send waiting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Queued {
    /// When it was queued (ms).
    at: u64,
    send: FeedbackSubmit,
}

/// What one try at control came to.
enum Try {
    Taken(FeedbackAck),
    Refused(StatusCode, FeedbackRefused),
    /// A signature control didn't take (401).
    Unsigned,
    Unreachable(String),
}

static NOW: OnceLock<Arc<Feedback>> = OnceLock::new();

/// The daemon's sender, which sends what was queued before and keeps
/// sending what's queued from now on.
pub fn start(dir: &Path, signer: Arc<dyn Signer>) -> Arc<Feedback> {
    let url = consent::url_in(&|k| std::env::var(k).ok());
    let f = Feedback::new(dir, url, signer, Backoff::default(), POW_DIFFICULTY);
    if NOW.set(f.clone()).is_ok() {
        crate::task::spawn(f.clone().run());
    }
    f
}

/// The running daemon's sender for `dir`; else (a test's daemon) one of its
/// own, whose queue waits for the next start.
pub fn sender(dir: &Path, signer: Arc<dyn Signer>) -> Arc<Feedback> {
    match NOW.get() {
        Some(f) if f.dir == dir => f.clone(),
        _ => {
            let url = consent::url_in(&|k| std::env::var(k).ok());
            Feedback::new(dir, url, signer, Backoff::default(), POW_DIFFICULTY)
        }
    }
}

impl Feedback {
    pub fn new(dir: &Path, url: String, signer: Arc<dyn Signer>, backoff: Backoff, difficulty: u8) -> Arc<Self> {
        let http = crate::control::control_http().timeout(Duration::from_secs(20)).build().expect("http client");
        Arc::new(Feedback {
            dir: dir.to_owned(),
            url: url.trim_end_matches('/').to_owned(),
            http,
            signer,
            backoff,
            difficulty,
            queue: Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        })
    }

    /// Send it now; queued if control can't be reached.
    pub async fn send(&self, send: FeedbackSubmit) -> Sent {
        if let Err(why) = send.check() {
            return Sent::Refused(StatusCode::BAD_REQUEST, check_words(why));
        }
        match self.attempt(&send).await {
            Ok(ack) => {
                info!(id = %ack.id, "feedback sent");
                Sent::Taken(ack)
            }
            Err(Try::Refused(code, r)) => Sent::Refused(code, refused_words(&r)),
            Err(Try::Unreachable(why)) => {
                info!(why, "control couldn't be reached: feedback queued");
                if let Err(e) = self.enqueue(send) {
                    warn!(error = %e, "couldn't queue feedback");
                    return Sent::Refused(
                        StatusCode::BAD_GATEWAY,
                        "Arugula's cloud couldn't be reached, and this machine couldn't keep the feedback to send later: please try again in a while.".into(),
                    );
                }
                self.wake.notify_one();
                Sent::Queued
            }
            Err(Try::Taken(_) | Try::Unsigned) => unreachable!("attempt answers these as Ok, or unsigned"),
        }
    }

    /// Signed if it can be, else unsigned with a stamp.
    async fn attempt(&self, send: &FeedbackSubmit) -> Result<FeedbackAck, Try> {
        let mut send = send.clone();
        send.who = Default::default();
        if self.signer.joined().is_some_and(|(url, _)| same(&url, &self.url)) {
            let body = serde_json::to_vec(&send).expect("a send serializes");
            if let Some(sig) = self.signer.sign_post(FEEDBACK, &body) {
                match self.post(body, Some(sig)).await {
                    Try::Taken(ack) => return Ok(ack),
                    Try::Unsigned => info!("control refused this machine's signature on feedback: sending it unsigned"),
                    other => return Err(other),
                }
            }
        }
        send.who.install = Some(self.install().map_err(|e| Try::Unreachable(format!("no install id: {e}")))?);
        let mut difficulty = self.difficulty;
        let mut again = true;
        loop {
            let digest = send.digest();
            let stamp = tokio::task::spawn_blocking(move || FeedbackStamp::solve(&digest, difficulty))
                .await
                .map_err(|e| Try::Unreachable(format!("solving the stamp: {e}")))?;
            send.who.stamp = stamp;
            let body = serde_json::to_vec(&send).expect("a send serializes");
            match self.post(body, None).await {
                Try::Taken(ack) => return Ok(ack),
                // Harder, once, and no harder than a sender works at.
                Try::Refused(_, r)
                    if again
                        && (r.error == POW_BAD || r.error == POW_MISSING)
                        && r.difficulty.is_some_and(|d| d > difficulty && d <= MAX_POW_DIFFICULTY) =>
                {
                    difficulty = r.difficulty.unwrap_or(difficulty);
                    again = false;
                    debug!(difficulty, "control asks a harder stamp for feedback");
                }
                other => return Err(other),
            }
        }
    }

    async fn post(&self, body: Vec<u8>, sig: Option<String>) -> Try {
        let signed = sig.is_some();
        let mut r = self
            .http
            .post(format!("{}{FEEDBACK}", self.url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(sig) = sig {
            r = r.header(crate::control::AUTH, sig);
        }
        let res = match r.send().await {
            Ok(res) => res,
            Err(e) => return Try::Unreachable(e.to_string()),
        };
        let code = res.status();
        if code.is_success() {
            return match res.json::<FeedbackAck>().await {
                Ok(ack) => Try::Taken(ack),
                // Taken, but its id didn't read: it isn't sent again.
                Err(e) => {
                    warn!(error = %e, "control took feedback with an answer that doesn't read");
                    Try::Taken(FeedbackAck { id: String::new(), shown: "(no number)".into() })
                }
            };
        }
        if signed && code == StatusCode::UNAUTHORIZED {
            return Try::Unsigned;
        }
        // No such route yet, or control having a bad time: try later.
        if code.is_server_error() || matches!(code, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED) {
            return Try::Unreachable(format!("control answered {code}"));
        }
        match res.json::<FeedbackRefused>().await {
            Ok(r) => Try::Refused(code, r),
            Err(_) => {
                Try::Refused(code, FeedbackRefused { error: code.to_string(), difficulty: None, retry_after: None })
            }
        }
    }

    /// The install id an unsigned send carries: telemetry's while it's on,
    /// else this machine's own for feedback, made the first time.
    fn install(&self) -> std::io::Result<String> {
        if let Some(i) = consent::enabled(&self.dir).install() {
            return Ok(i.to_owned());
        }
        let path = self.dir.join(INSTALL_FILE);
        if let Ok(i) = std::fs::read_to_string(&path).map(|t| t.trim().to_owned())
            && consent::is_install(&i)
        {
            return Ok(i);
        }
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).map_err(|e| std::io::Error::other(format!("no random source: {e}")))?;
        let i: String = b.iter().map(|b| format!("{b:02x}")).collect();
        crate::store::write_atomic(&path, format!("{i}\n").as_bytes())?;
        Ok(i)
    }

    fn queued(&self) -> Vec<Queued> {
        std::fs::read(self.dir.join(QUEUE_FILE))
            .ok()
            .and_then(|b| {
                serde_json::from_slice(&b).map_err(|e| warn!(error = %e, "{QUEUE_FILE} doesn't parse: dropped")).ok()
            })
            .unwrap_or_default()
    }

    fn save(&self, q: &[Queued]) -> std::io::Result<()> {
        let path = self.dir.join(QUEUE_FILE);
        if q.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        crate::store::write_atomic(&path, &serde_json::to_vec_pretty(q).expect("the queue serializes"))
    }

    fn enqueue(&self, send: FeedbackSubmit) -> std::io::Result<()> {
        let _held = self.queue.lock().unwrap();
        let mut q = self.queued();
        q.push(Queued { at: now_ms(), send });
        let over = q.len().saturating_sub(MAX_QUEUED);
        q.drain(..over);
        self.save(&q)
    }

    /// Sends what's queued, oldest first, until control can't be reached.
    /// Whether nothing is left.
    pub async fn flush(&self) -> bool {
        loop {
            let first = {
                let _held = self.queue.lock().unwrap();
                self.queued().into_iter().next()
            };
            let Some(first) = first else { return true };
            let keep = if now_ms().saturating_sub(first.at) > KEEP_MS {
                info!("feedback queued a week ago never reached control: dropped");
                false
            } else {
                match self.attempt(&first.send).await {
                    Ok(ack) => {
                        info!(id = %ack.id, "queued feedback sent");
                        false
                    }
                    // Too many just now: later, as for no answer.
                    Err(Try::Refused(_, r)) if r.error == RATE_LIMITED => true,
                    Err(Try::Refused(_, r)) => {
                        warn!(error = r.error, "control refused queued feedback: dropped");
                        false
                    }
                    Err(_) => true,
                }
            };
            if keep {
                return false;
            }
            let _held = self.queue.lock().unwrap();
            let mut q = self.queued();
            if q.first() == Some(&first) {
                q.remove(0);
            }
            if let Err(e) = self.save(&q) {
                warn!(error = %e, "couldn't write {QUEUE_FILE}");
                return false;
            }
        }
    }

    /// The daemon's loop: what's queued goes when it can, with backoff.
    pub async fn run(self: Arc<Self>) {
        let mut wait = self.backoff.first;
        loop {
            if self.queued().is_empty() {
                self.wake.notified().await;
                wait = self.backoff.first;
            }
            tokio::time::sleep(wait).await;
            wait = match self.flush().await {
                true => self.backoff.first,
                false => (wait * 2).min(self.backoff.most),
            };
        }
    }
}

/// What a person or agent is told of control's refusal.
pub fn refused_words(r: &FeedbackRefused) -> String {
    match r.error.as_str() {
        RATE_LIMITED => {
            let when = match r.retry_after {
                Some(s) if s >= 90 => format!("in about {} minutes", s.div_ceil(60)),
                Some(_) => "in a minute".into(),
                None => "a little later".into(),
            };
            format!(
                "Arugula has had a lot of feedback from here lately, so it isn't taking more just now. Please try again {when}. Thanks for writing."
            )
        }
        ANONYMOUS_OFF => "Arugula isn't taking feedback from machines that aren't joined to Arugula cloud just now. Join this one (`arugula join`), or send it while signed in at app.arugula.io. Thanks for writing.".into(),
        TOO_LARGE => check_words(TOO_LARGE),
        POW_BAD | POW_MISSING => "Arugula's cloud asked more work of this machine than it does to send feedback unsigned. Please try again later, or join this machine to Arugula cloud (`arugula join`).".into(),
        other => format!("Arugula's cloud didn't take the feedback ({other})."),
    }
}

/// What a send that `FeedbackSubmit::check` refuses is told.
fn check_words(why: &str) -> String {
    match why {
        TOO_LARGE => format!(
            "That's longer than Arugula takes: a title of at most {MAX_TITLE} characters and a body of at most {} KB.",
            MAX_BODY / 1024
        ),
        why => format!("That can't be sent: {why}."),
    }
}

fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches('/').eq_ignore_ascii_case(b.trim_end_matches('/'))
}

#[cfg(test)]
mod tests;
