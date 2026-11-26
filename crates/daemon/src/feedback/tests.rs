//! Feedback against a fake control (#1216): signed or stamped, a harder
//! stamp when asked, queued while control is away, and control's refusals
//! as words of our own.

use std::{collections::VecDeque, future::IntoFuture};

use arugula_control_wire::feedback::{FeedbackKind, FeedbackSource, FeedbackSurface};
use axum::{Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};

use super::*;

/// What the fake got: each send, and its signature if any.
type Got = Arc<Mutex<Vec<(FeedbackSubmit, Option<String>)>>>;
/// What the fake answers next, in order; with none left, it takes the send.
type Script = Arc<Mutex<VecDeque<(u16, serde_json::Value)>>>;

struct Rig {
    dir: PathBuf,
    got: Got,
    script: Script,
    url: String,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("arugula-feedback-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let got: Got = Default::default();
    let script: Script = Default::default();
    let app = Router::new()
        .route(
            FEEDBACK,
            post(|State((got, script)): State<(Got, Script)>, headers: HeaderMap, body: String| async move {
                let sig = headers.get(crate::control::AUTH).map(|v| v.to_str().unwrap().to_owned());
                let send: FeedbackSubmit = serde_json::from_str(&body).unwrap();
                let next = script.lock().unwrap().pop_front();
                if let Some((code, v)) = next {
                    return (StatusCode::from_u16(code).unwrap(), Json(v)).into_response();
                }
                got.lock().unwrap().push((send, sig));
                Json(serde_json::json!({ "id": "fbk_1", "shown": "F-7K2Q9X" })).into_response()
            }),
        )
        .with_state((got.clone(), script.clone()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    crate::task::spawn(axum::serve(l, app).into_future());
    Rig { dir, got, script, url }
}

/// Not joined, or joined to `url` and signing.
struct Joined(Option<String>);

impl Signer for Joined {
    fn joined(&self) -> Option<(String, String)> {
        self.0.clone().map(|u| (u, "cert".into()))
    }

    fn sign_post(&self, path: &str, _: &[u8]) -> Option<String> {
        self.0.as_ref().map(|_| format!("sig {path}"))
    }
}

const FAST: Backoff = Backoff { first: Duration::from_millis(20), most: Duration::from_millis(80) };
/// Cheap stamps: the rules are the same at any difficulty.
const EASY: u8 = 4;

impl Rig {
    fn sender(&self, joined: bool) -> Arc<Feedback> {
        let signer = Arc::new(Joined(joined.then(|| self.url.clone())));
        Feedback::new(&self.dir, self.url.clone(), signer, FAST, EASY)
    }

    fn answer(&self, code: u16, v: serde_json::Value) {
        self.script.lock().unwrap().push_back((code, v));
    }
}

fn send() -> FeedbackSubmit {
    FeedbackSubmit {
        kind: FeedbackKind::Bug,
        title: "Panes flicker".into(),
        body: "When I resize.".into(),
        contact_ok: false,
        source: FeedbackSource::Agent,
        surface: FeedbackSurface::Mcp,
        agent: None,
        context: Default::default(),
        who: Default::default(),
    }
}

#[tokio::test]
async fn an_unjoined_machine_sends_its_install_id_and_a_stamp_that_holds() {
    let r = rig("unsigned").await;
    let sent = r.sender(false).send(send()).await;
    assert_eq!(sent, Sent::Taken(FeedbackAck { id: "fbk_1".into(), shown: "F-7K2Q9X".into() }));
    let got = r.got.lock().unwrap().clone();
    let (f, sig) = &got[0];
    assert!(sig.is_none());
    let install = f.who.install.clone().unwrap();
    assert!(consent::is_install(&install));
    assert!(f.who.stamp.unwrap().holds(&f.digest(), EASY));
    // The same id next time.
    drop(got);
    r.sender(false).send(send()).await;
    assert_eq!(r.got.lock().unwrap()[1].0.who.install.as_deref(), Some(install.as_str()));
}

#[tokio::test]
async fn a_joined_machine_signs_and_sends_no_install_id() {
    let r = rig("signed").await;
    assert!(matches!(r.sender(true).send(send()).await, Sent::Taken(_)));
    let (f, sig) = r.got.lock().unwrap()[0].clone();
    assert_eq!(sig.as_deref(), Some("sig /api/feedback"));
    assert_eq!(f.who, Default::default());
}

#[tokio::test]
async fn a_refused_signature_goes_again_unsigned() {
    let r = rig("stale").await;
    r.answer(401, serde_json::json!({ "error": "unauthorized" }));
    assert!(matches!(r.sender(true).send(send()).await, Sent::Taken(_)));
    let (f, sig) = r.got.lock().unwrap()[0].clone();
    assert!(sig.is_none() && f.who.stamp.is_some());
}

#[tokio::test]
async fn a_harder_stamp_is_solved_once() {
    let r = rig("harder").await;
    r.answer(403, serde_json::json!({ "error": POW_BAD, "difficulty": 6 }));
    assert!(matches!(r.sender(false).send(send()).await, Sent::Taken(_)));
    let f = r.got.lock().unwrap()[0].0.clone();
    assert!(f.who.stamp.unwrap().holds(&f.digest(), 6));

    // Asked twice, it stops: once is all.
    r.answer(403, serde_json::json!({ "error": POW_BAD, "difficulty": 6 }));
    r.answer(403, serde_json::json!({ "error": POW_BAD, "difficulty": 8 }));
    let Sent::Refused(code, why) = r.sender(false).send(send()).await else { panic!("refused") };
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert!(why.contains("join this machine"), "{why}");
    assert!(r.script.lock().unwrap().is_empty());

    // More than a sender works at: not tried.
    r.answer(403, serde_json::json!({ "error": POW_BAD, "difficulty": MAX_POW_DIFFICULTY + 1 }));
    assert!(matches!(r.sender(false).send(send()).await, Sent::Refused(..)));
    assert_eq!(r.got.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn refusals_are_polite_words() {
    let r = rig("refused").await;
    r.answer(429, serde_json::json!({ "error": RATE_LIMITED, "retry_after": 600 }));
    let Sent::Refused(code, why) = r.sender(false).send(send()).await else { panic!("refused") };
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    assert!(why.contains("try again in about 10 minutes") && !why.contains(RATE_LIMITED), "{why}");
    r.answer(403, serde_json::json!({ "error": ANONYMOUS_OFF }));
    let Sent::Refused(_, why) = r.sender(false).send(send()).await else { panic!("refused") };
    assert!(why.contains("arugula join") && !why.contains(ANONYMOUS_OFF), "{why}");
    // Too long never leaves the machine.
    let mut long = send();
    long.title = "x".repeat(MAX_TITLE + 1);
    let Sent::Refused(code, why) = r.sender(false).send(long).await else { panic!("refused") };
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(why.contains("at most 200 characters"), "{why}");
    assert!(r.got.lock().unwrap().is_empty() && !r.dir.join(QUEUE_FILE).exists());
}

#[tokio::test]
async fn while_control_is_away_it_queues_and_sends_later() {
    let r = rig("queued").await;
    r.answer(503, serde_json::json!({}));
    r.answer(404, serde_json::json!({}));
    let f = r.sender(false);
    assert_eq!(f.send(send()).await, Sent::Queued);
    assert!(r.dir.join(QUEUE_FILE).exists());
    let queued = f.queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].send.who, Default::default(), "queued without an id or stamp");
    // The loop's first try finds no route yet (404), its next sends it.
    crate::task::spawn(f.clone().run());
    // Wait for the queue to empty, not for the fake to see the send: the
    // fake keeps it before answering, and the loop empties the queue only
    // once the answer is back (#1389).
    for _ in 0..1000 {
        if !r.dir.join(QUEUE_FILE).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!r.dir.join(QUEUE_FILE).exists());
    let got = r.got.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert!(got[0].0.who.stamp.is_some());
}

#[tokio::test]
async fn nobody_listening_is_queued_and_a_week_old_send_is_dropped() {
    let r = rig("away").await;
    let f = Feedback::new(&r.dir, "http://127.0.0.1:9".into(), Arc::new(Joined(None)), FAST, EASY);
    assert_eq!(f.send(send()).await, Sent::Queued);
    assert!(!f.flush().await, "still away");
    let mut q = f.queued();
    q[0].at -= KEEP_MS + 1;
    f.save(&q).unwrap();
    assert!(f.flush().await);
    assert!(!r.dir.join(QUEUE_FILE).exists());
}
