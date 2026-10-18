//! Telemetry against a fake endpoint: what reaches it, when, and signed or
//! not (M101's done-when).

use std::sync::atomic::AtomicU16;

use axum::{Router, extract::State, http::HeaderMap, routing::post};

use super::*;

/// What the fake endpoint got: each report, and its signature if any.
type Got = Arc<Mutex<Vec<(Report, Option<String>)>>>;
/// The status the fake answers a signed report with, then an unsigned one.
type Status = Arc<Mutex<(u16, u16)>>;

struct Rig {
    dir: PathBuf,
    got: Got,
    status: Status,
    url: String,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("arugula-tel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let got: Got = Default::default();
    let status = Arc::new(Mutex::new((200, 200)));
    let app = Router::new()
        .route(
            TELEMETRY,
            post(|State((got, status)): State<(Got, Status)>, headers: HeaderMap, body: String| async move {
                let sig = headers.get(crate::control::AUTH).map(|v| v.to_str().unwrap().to_owned());
                let (signed, unsigned) = *status.lock().unwrap();
                let code = if sig.is_some() { signed } else { unsigned };
                if code == 200 {
                    got.lock().unwrap().push((serde_json::from_str(&body).unwrap(), sig));
                }
                axum::http::StatusCode::from_u16(code).unwrap()
            }),
        )
        .with_state((got.clone(), status.clone()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(axum::serve(l, app).into_future());
    Rig { dir, got, status, url }
}

/// No waiting for `started`, and no retry delay.
const FAST: Timing = Timing {
    started_after: Duration::ZERO,
    tick: Duration::from_secs(3600),
    day: Duration::from_secs(24 * 3600),
    retry: Duration::ZERO,
};

impl Rig {
    /// A daemon's telemetry here, with `env` and reports to the fake.
    fn telemetry(&self, env: &[(&'static str, &'static str)], timing: Timing) -> Arc<Telemetry> {
        let url = self.url.clone();
        let env: Vec<_> = env.to_vec();
        let env: Env = Arc::new(move |k: &str| {
            if k == consent::URL_ENV {
                return Some(url.clone());
            }
            env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        });
        Telemetry::new(&self.dir, env, timing, Channel::Source)
    }

    fn events(&self) -> Vec<Event> {
        self.got.lock().unwrap().iter().flat_map(|(r, _)| r.events.clone()).collect()
    }

    fn yes(&self) {
        consent::set(&self.dir, true, "cli", now_ms()).unwrap();
    }
}

#[tokio::test]
async fn with_no_file_nothing_is_ever_sent() {
    let r = rig("none").await;
    let t = r.telemetry(&[], FAST);
    t.count(|c| c.panes_opened += 1);
    for _ in 0..3 {
        t.look().await;
    }
    t.save();
    assert!(r.got.lock().unwrap().is_empty());
    assert!(!r.dir.join(COUNTS_FILE).exists(), "nothing counted, nothing kept");
}

/// Before a yes, "what's sent" is this machine's first report, marked as a
/// preview: its own version and platform, no counts, no install id.
#[tokio::test]
async fn the_preview_shows_what_a_yes_would_send() {
    let r = rig("preview").await;
    let t = r.telemetry(&[], FAST);
    let p = t.info().preview.expect("a preview while not asked");
    assert_eq!(p["preview_only"], true);
    assert_eq!(p["install"], PREVIEW_INSTALL);
    assert_eq!(p["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!((p["channel"].as_str(), p["control"].as_str()), (Some("source"), Some("none")));
    let report: Report = serde_json::from_value(p).unwrap();
    assert!(report.check().is_err(), "a preview can't be taken for a report");
    let names: Vec<_> = report.events.iter().map(|e| serde_json::to_value(e).unwrap()["event"].clone()).collect();
    assert_eq!(names, ["opted-in", "started", "daily"]);
    assert_eq!(report.events[2], Event::Daily(Counts::default()), "nothing is counted before a yes");

    r.yes();
    let p = t.info().preview.unwrap();
    assert!(p.get("preview_only").is_none(), "on, it's the real next report");
    assert_eq!(p["install"].as_str(), consent::read(&r.dir).unwrap().install.as_deref());
}

#[tokio::test]
async fn after_a_yes_opted_in_then_started_arrive() {
    let r = rig("yes").await;
    let t = r.telemetry(&[], Timing { started_after: Duration::from_millis(300), ..FAST });
    r.yes();
    t.answered();
    t.look().await;
    assert_eq!(r.events(), [Event::OptedIn { via: Surface::Cli }], "opted-in at once");
    tokio::time::sleep(Duration::from_millis(350)).await;
    t.look().await;
    assert_eq!(r.events(), [Event::OptedIn { via: Surface::Cli }, Event::Started { service: false }]);
    let report = r.got.lock().unwrap()[0].0.clone();
    assert_eq!(Some(report.install.as_str()), consent::read(&r.dir).unwrap().install.as_deref());
    assert_eq!((report.channel, report.control), (Channel::Source, ControlKind::None));
    assert!(report.check().is_ok());
    t.look().await;
    assert_eq!(r.events().len(), 2, "each once");
}

#[tokio::test]
async fn do_not_track_sends_nothing_even_with_a_yes() {
    let r = rig("dnt").await;
    r.yes();
    for env in [("DO_NOT_TRACK", "1"), (consent::ENV, "off"), ("CI", "true")] {
        let t = r.telemetry(&[env], FAST);
        t.count(|c| c.panes_opened += 1);
        t.look().await;
        assert!(!t.info().on, "{env:?}");
    }
    assert!(r.got.lock().unwrap().is_empty());
}

#[tokio::test]
async fn off_deletes_the_install_id_and_the_queue() {
    let r = rig("off").await;
    *r.status.lock().unwrap() = (503, 503);
    r.yes();
    let t = r.telemetry(&[], FAST);
    t.count(|c| c.panes_opened += 1);
    t.look().await;
    t.save();
    let kept: Kept = serde_json::from_slice(&std::fs::read(r.dir.join(COUNTS_FILE)).unwrap()).unwrap();
    assert_eq!(kept.queue.len(), 2, "kept to try again: {kept:?}");

    consent::set(&r.dir, false, "cli", now_ms()).unwrap();
    t.answered();
    assert_eq!(consent::read(&r.dir).unwrap().install, None);
    assert!(!r.dir.join(COUNTS_FILE).exists());
    *r.status.lock().unwrap() = (200, 200);
    t.count(|c| c.panes_opened += 1);
    t.look().await;
    t.save();
    assert!(r.got.lock().unwrap().is_empty(), "the queue went with the no");
    assert!(!r.dir.join(COUNTS_FILE).exists(), "and nothing is counted after it");
}

#[tokio::test]
async fn a_failed_report_is_tried_again_and_a_week_old_one_dropped() {
    let r = rig("retry").await;
    *r.status.lock().unwrap() = (429, 429);
    r.yes();
    let t = r.telemetry(&[], Timing { retry: Duration::from_secs(3600), ..FAST });
    t.look().await;
    *r.status.lock().unwrap() = (200, 200);
    t.look().await;
    assert!(r.got.lock().unwrap().is_empty(), "not again before the next try");
    t.live.lock().unwrap().wait_until = None;
    t.look().await;
    assert_eq!(r.events().len(), 2, "sent at the next try");

    *r.status.lock().unwrap() = (503, 503);
    t.live
        .lock()
        .unwrap()
        .kept
        .queue
        .push(Queued { at: now_ms() - WEEK_MS - 1, event: Event::Started { service: true } });
    t.live.lock().unwrap().wait_until = None;
    t.look().await;
    assert!(t.live.lock().unwrap().kept.queue.is_empty(), "a week old is dropped");
}

#[tokio::test]
async fn a_joined_daemons_report_is_signed_and_an_unjoined_ones_isnt() {
    static N: AtomicU16 = AtomicU16::new(0);
    let r = rig("signed").await;
    r.yes();
    let n = N.fetch_add(1, Ordering::Relaxed);
    let at = |url: &str| {
        let d = r.dir.join(format!("control-{n}-{}", url.len()));
        std::fs::create_dir_all(&d).unwrap();
        crate::control::tests::joined_at(&d, url)
    };
    let sig = |t: &Arc<Telemetry>| {
        let r = &r;
        let t = t.clone();
        async move {
            r.got.lock().unwrap().clear();
            t.look().await;
            r.got.lock().unwrap().pop().map(|(rep, sig)| (rep.control, sig.is_some()))
        }
    };

    let joined = r.telemetry(&[], FAST);
    let _ = joined.signer.set(at(&r.url));
    assert!(joined.info().signed);
    assert_eq!(sig(&joined).await, Some((ControlKind::SelfHosted, true)), "joined to the control it sends to");

    let elsewhere = r.telemetry(&[], FAST);
    let _ = elsewhere.signer.set(at("https://someone-elses.example"));
    elsewhere.live.lock().unwrap().kept = Kept::default();
    assert_eq!(sig(&elsewhere).await, Some((ControlKind::SelfHosted, false)), "someone else's control isn't signed to");

    let alone = r.telemetry(&[], FAST);
    assert_eq!(sig(&alone).await, Some((ControlKind::None, false)));

    // A stale enrolment: control refuses its signature, so it goes again
    // unsigned, and stays unsigned.
    *r.status.lock().unwrap() = (401, 200);
    let stale = r.telemetry(&[], FAST);
    let _ = stale.signer.set(at(&r.url));
    assert_eq!(sig(&stale).await, Some((ControlKind::SelfHosted, false)));
    stale.live.lock().unwrap().kept.queue.push(Queued { at: now_ms(), event: Event::Started { service: false } });
    assert_eq!(sig(&stale).await, Some((ControlKind::SelfHosted, false)), "no signed try first");
}

#[tokio::test]
async fn counts_survive_a_restart() {
    let r = rig("restart").await;
    r.yes();
    let t = r.telemetry(&[], FAST);
    t.look().await;
    t.count(|c| c.panes_opened += 2);
    t.save();
    drop(t);

    let after = r.telemetry(&[], FAST);
    let kept = after.live.lock().unwrap().kept.clone();
    assert_eq!(kept.counts.panes_opened, 2);
    after.count(|c| *c.blocks_opened.entry(BlockKind::Browser).or_default() += 1);
    after.live.lock().unwrap().kept.since = 0;
    after.look().await;
    let daily = r.events().into_iter().find_map(|e| if let Event::Daily(c) = e { Some(c) } else { None });
    let daily = daily.expect("a day's counts");
    assert_eq!(daily.panes_opened, 2);
    assert_eq!(daily.blocks_opened[&BlockKind::Browser], 1);
    assert_eq!(r.events().iter().filter(|e| matches!(e, Event::OptedIn { .. })).count(), 1, "the same install");
}

#[test]
fn what_it_names() {
    assert_eq!(runtime("claude", &[]), AgentRuntime::Claude);
    assert_eq!(runtime("managoat", &[]), AgentRuntime::Cloud);
    assert_eq!(runtime("acp", &["/usr/bin/gemini".into(), "--acp".into()]), AgentRuntime::Gemini);
    assert_eq!(runtime("acp", &["my-agent".into()]), AgentRuntime::Other);
    assert_eq!(block_kind(BlockType::Forge, &serde_json::json!({ "kind": "issue" })), BlockKind::Issue);
    assert_eq!(block_kind(BlockType::Diff, &Value::Null), BlockKind::Review);
    let home = Path::new("/home/me");
    assert_eq!(channel(Path::new("/home/me/.local/bin/arugulad"), home), Channel::Script);
    assert_eq!(channel(Path::new("/opt/homebrew/Cellar/arugula/0.42.0/bin/arugulad"), home), Channel::Homebrew);
    assert_eq!(channel(Path::new("/Applications/Arugula.app/Contents/MacOS/arugulad"), home), Channel::Desktop);
    assert_eq!(channel(Path::new("/home/me/src/arugula/target/release/arugulad"), home), Channel::Source);
    assert_eq!(channel(Path::new("/usr/bin/arugulad"), home), Channel::Other);
    assert!(!Telemetry::from_env(Path::new("/nonexistent"), home).info().on, "a test build's is off");
}
