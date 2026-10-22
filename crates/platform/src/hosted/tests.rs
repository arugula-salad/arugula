//! Hosted workspaces (M179) through the platform's own routes and its
//! worker, against a fake substrate and the test hub: making a box and
//! enrolling it, a lost create, failing closed, ticket mode, waking,
//! rebuilding and deleting, and who may do what. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering::SeqCst},
    },
    time::Duration,
};

use arugula_e2e::{now_ms, team::TeamRole};
use futures_util::{FutureExt as _, future::BoxFuture};
use serde_json::{Value, json};
use std::panic::AssertUnwindSafe;

use super::{Config, EnrollBy, enroll, worker};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    compute::{self, Create, Created, Paused, Resumed, Sandbox, State, Substrate},
};

/// A provider in memory. `lose_next`: the next create makes its box but
/// answers nothing; `refuse_next`: it makes nothing and says no. A resume
/// is counted (`resumes`), takes `resume_ms`, and fails with `resume_error`
/// while it's set (M182). `calls`: each resume, pause and hold, in order
/// (M181).
#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) boxes: Mutex<BTreeMap<String, Sandbox>>,
    creates: Mutex<Vec<Create>>,
    deleted: Mutex<Vec<String>>,
    n: AtomicU32,
    lose_next: AtomicBool,
    refuse_next: AtomicBool,
    pub(super) resumes: AtomicU32,
    pub(super) resume_ms: AtomicU32,
    pub(super) resume_error: Mutex<Option<compute::Error>>,
    pub(crate) calls: Mutex<Vec<String>>,
    /// Each box's egress rules as last set (M185).
    pub(super) rules: Mutex<BTreeMap<String, compute::Rules>>,
}

impl Fake {
    /// Each resume, pause and hold since the last asked, as `pause sbx0`
    /// or `hold sbx0 600s`.
    pub(crate) fn calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    pub(crate) fn boxes(&self) -> Vec<Sandbox> {
        self.boxes.lock().unwrap().values().cloned().collect()
    }

    pub(super) fn creates(&self) -> Vec<Create> {
        self.creates.lock().unwrap().clone()
    }

    /// Its timeout passed: the box paused itself.
    pub(super) fn pause_all(&self) {
        for b in self.boxes.lock().unwrap().values_mut() {
            b.state = State::Paused;
        }
    }
}

fn ready<T: Send + 'static>(v: compute::Result<T>) -> BoxFuture<'static, compute::Result<T>> {
    Box::pin(async move { v })
}

impl Substrate for Fake {
    fn create<'a>(&'a self, b: &'a Create) -> BoxFuture<'a, compute::Result<Created>> {
        self.creates.lock().unwrap().push(b.clone());
        if self.refuse_next.swap(false, SeqCst) {
            return ready(Err(compute::Error::Refused { status: 400, code: None }));
        }
        let id = format!("sbx{}", self.n.fetch_add(1, SeqCst));
        self.boxes.lock().unwrap().insert(
            id.clone(),
            Sandbox {
                id: id.clone(),
                template: b.template.clone(),
                state: State::Running,
                metadata: b.metadata.clone(),
                started_at: String::new(),
                end_at: String::new(),
                pauses_on_timeout: Some(true),
                cpus: Some(2),
                memory_mb: Some(2048),
            },
        );
        if self.lose_next.swap(false, SeqCst) {
            return ready(Err(compute::Error::Unreachable));
        }
        ready(Ok(Created { id, template: b.template.clone() }))
    }

    fn resume<'a>(&'a self, id: &'a str, _: Duration) -> BoxFuture<'a, compute::Result<Resumed>> {
        Box::pin(async move {
            self.resumes.fetch_add(1, SeqCst);
            self.calls.lock().unwrap().push(format!("resume {id}"));
            let ms = self.resume_ms.load(SeqCst);
            if ms > 0 {
                tokio::time::sleep(Duration::from_millis(ms.into())).await;
            }
            if let Some(e) = self.resume_error.lock().unwrap().clone() {
                return Err(e);
            }
            let mut boxes = self.boxes.lock().unwrap();
            match boxes.get_mut(id) {
                None => Err(compute::Error::NotFound),
                Some(b) if b.state == State::Paused => {
                    b.state = State::Running;
                    Ok(Resumed::Woke)
                }
                Some(_) => Ok(Resumed::Running),
            }
        })
    }

    fn pause<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<Paused>> {
        self.calls.lock().unwrap().push(format!("pause {id}"));
        let mut boxes = self.boxes.lock().unwrap();
        ready(match boxes.get_mut(id) {
            None => Err(compute::Error::NotFound),
            Some(b) => Ok(if std::mem::replace(&mut b.state, State::Paused) == State::Paused {
                Paused::Already
            } else {
                Paused::Paused
            }),
        })
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<()>> {
        self.boxes.lock().unwrap().remove(id);
        self.deleted.lock().unwrap().push(id.into());
        ready(Ok(()))
    }

    fn status<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<Option<Sandbox>>> {
        ready(Ok(self.boxes.lock().unwrap().get(id).cloned()))
    }

    fn list<'a>(&'a self, m: &'a BTreeMap<String, String>) -> BoxFuture<'a, compute::Result<Vec<Sandbox>>> {
        let boxes = self.boxes.lock().unwrap();
        ready(Ok(boxes.values().filter(|b| m.iter().all(|(k, v)| b.metadata.get(k) == Some(v))).cloned().collect()))
    }

    /// As E2B's: a paused box (or none) is `NotFound`, never woken.
    fn hold<'a>(&'a self, id: &'a str, timeout: Duration) -> BoxFuture<'a, compute::Result<()>> {
        self.calls.lock().unwrap().push(format!("hold {id} {}s", timeout.as_secs()));
        let boxes = self.boxes.lock().unwrap();
        ready(match boxes.get(id) {
            Some(b) if b.state == State::Running => Ok(()),
            _ => Err(compute::Error::NotFound),
        })
    }

    fn set_rules<'a>(&'a self, id: &'a str, rules: &'a compute::Rules) -> BoxFuture<'a, compute::Result<()>> {
        if !self.boxes.lock().unwrap().get(id).is_some_and(|b| b.state == State::Running) {
            return ready(Err(compute::Error::NotFound));
        }
        self.rules.lock().unwrap().insert(id.into(), rules.clone());
        ready(Ok(()))
    }
}

/// alice owns team t1, bob edits it, vic views it; olga is elsewhere.
/// The platform has the fake provider, template `tpl`, and lets anyone
/// make workspaces but carol.
pub(crate) async fn serve(enroll: EnrollBy) -> (Arc<TestHub>, Arc<Fake>, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    hub.team("t2", &[("olga", TeamRole::Owner), ("carol", TeamRole::Owner)]);
    let fake = Arc::new(Fake::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.compute = Some(fake.clone());
    p.hosted = Config {
        template: Some("tpl".into()),
        enroll,
        accounts: vec!["alice".into(), "bob".into(), "vic".into(), "olga".into()],
        cipher: None,
        ..Default::default()
    };
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, fake, app, base, project)
}

/// Every step due at `now`.
pub(crate) async fn drive(app: &Platform, now: u64) {
    for _ in 0..50 {
        if !worker::tick_at(app, now).await.unwrap() {
            return;
        }
    }
    panic!("the worker never settled");
}

pub(crate) async fn make(base: &str, who: &str, project: &str, name: &str) -> (u16, Value) {
    let spec = json!({ "repos": ["https://github.com/acme/app"], "packages": ["ripgrep"], "setup": "make" });
    call(base, who, "POST", &format!("/api/projects/{project}/workspaces"), Some(json!({ "name": name, "spec": spec })))
        .await
}

pub(crate) async fn show(base: &str, project: &str, id: &str) -> Value {
    let (status, w) = call(base, "alice", "GET", &format!("/api/projects/{project}/workspaces/{id}"), None).await;
    assert_eq!(status, 200, "{w}");
    w
}

/// A member makes a workspace; its box is made from the template with the
/// workspace's metadata and nothing in its env; its enrollment is recorded
/// by sandbox id under the project's owner; it shows `ready` with its
/// machine once the hub says it enrolled. The sandbox id is never answered.
#[tokio::test]
async fn a_workspace_is_made_and_enrolls() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (status, w) = make(&base, "bob", &project, "dev").await;
    assert_eq!(status, 201, "{w}");
    let id = w["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("hw_") && id.len() == 35, "{id}");
    assert_eq!(w["state"], "creating");

    let now = now_ms();
    drive(&app, now).await;
    let made = fake.creates();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].template, "tpl");
    assert!(made[0].env.is_empty(), "a snapshot can't read its env: nothing goes in it");
    assert_eq!(made[0].metadata["arugula_workspace"], id);
    assert_eq!(made[0].metadata["arugula_project"], project);
    assert_eq!(made[0].metadata["arugula_template"], "tpl");
    assert_eq!(made[0].metadata["arugula_release"], env!("CARGO_PKG_VERSION"));
    let asked = hub.enrollments();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].owner.as_str(), asked[0].workspace.as_str(), asked[0].name.as_str()),
        ("team:t1", &*id, "dev")
    );
    assert_eq!(asked[0].sandbox.as_deref(), Some("sbx0"));
    let w = show(&base, &project, "dev").await;
    assert_eq!(w["state"], "enrolling");
    assert!(!w.to_string().contains("sbx0"), "the sandbox id is a bearer secret: {w}");

    hub.enroll(&id, "hosted-machine-1");
    drive(&app, now + 3_000).await;
    let w = show(&base, &project, &id).await;
    assert_eq!((w["state"].as_str(), w["machine"].as_str()), (Some("ready"), Some("hosted-machine-1")));
    assert_eq!(w["template"], "tpl");
    assert_eq!(w["rebuild_needed"], false);

    // Listed for every member, a viewer too.
    let (status, listed) = call(&base, "vic", "GET", &format!("/api/projects/{project}/workspaces"), None).await;
    assert_eq!(status, 200);
    assert_eq!(listed["workspaces"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains("sbx0"));
}

/// Who may do what: viewers list and open, owners and editors on the
/// allowlist make and rebuild, outsiders see nothing, and a control
/// without a provider has no hosted workspaces.
#[tokio::test]
async fn who_may_do_what() {
    let (_hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    assert_eq!(make(&base, "vic", &project, "dev").await.0, 403, "a viewer");
    assert_eq!(make(&base, "olga", &project, "dev").await.0, 404, "not a member");
    assert_eq!(make(&base, "alice", &project, "Not ok").await.0, 400);
    let bad = json!({ "name": "dev", "spec": { "repos": ["https://user:pw@github.com/acme/app"] } });
    let (status, _) = call(&base, "alice", "POST", &format!("/api/projects/{project}/workspaces"), Some(bad)).await;
    assert_eq!(status, 422, "credentials in a clone URL");
    let bad = json!({ "name": "dev", "spec": { "env": ["ARUGULA_HOSTED_TICKET"] } });
    let (status, _) = call(&base, "alice", "POST", &format!("/api/projects/{project}/workspaces"), Some(bad)).await;
    assert_eq!(status, 422, "a reserved env name");
    let (status, w) = make(&base, "alice", &project, "dev").await;
    assert_eq!(status, 201);
    assert_eq!(make(&base, "bob", &project, "dev").await.0, 409, "the name's taken");
    let id = w["id"].as_str().unwrap();
    let path = format!("/api/projects/{project}/workspaces/{id}");
    assert_eq!(call(&base, "vic", "GET", &path, None).await.0, 200);
    assert_eq!(call(&base, "olga", "GET", &path, None).await.0, 404);
    assert_eq!(call(&base, "vic", "DELETE", &path, None).await.0, 403);
    assert_eq!(call(&base, "vic", "POST", &format!("{path}/rebuild"), None).await.0, 403);
    // Another project's workspace isn't found through this one.
    let (status, _) = call(&base, "olga", "GET", &format!("/api/projects/personal/workspaces/{id}"), None).await;
    assert_eq!(status, 404);
    // Off the allowlist: carol can't make one in her own project.
    let carols = app.db.team_project("t2", "t2", 0).unwrap();
    assert_eq!(make(&base, "carol", &carols, "dev").await.0, 403);

    // The shell offers to delegate a project only where they're open.
    assert_eq!(call(&base, "alice", "GET", "/api/projects", None).await.1["hosted"], true);
    assert_eq!(call(&base, "carol", "GET", "/api/projects", None).await.1["hosted"], false);

    // Without a provider.
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner)]);
    let (_, base) = crate::chat_tests::serve(hub).await;
    assert_eq!(make(&base, "alice", "personal", "dev").await.0, 404);
    assert_eq!(call(&base, "alice", "GET", "/api/projects", None).await.1["hosted"], false);
}

/// A create whose answer was lost is found by its metadata, never made
/// again; one that made nothing fails once it's had time to show up, and
/// one the provider refused fails at once.
#[tokio::test]
async fn a_lost_create_is_found_not_repeated() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    fake.lose_next.store(true, SeqCst);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let now = now_ms();
    drive(&app, now).await;
    assert_eq!(show(&base, &project, &id).await["state"], "creating");
    drive(&app, now + 6_000).await;
    assert_eq!(fake.creates().len(), 1, "never sent twice");
    assert_eq!(show(&base, &project, &id).await["state"], "enrolling", "the box it made was found");
    assert_eq!(hub.enrollments()[0].sandbox.as_deref(), Some("sbx0"));

    // Made nothing, and answered nothing: given a minute, then failed.
    fake.lose_next.store(true, SeqCst);
    let (_, w) = make(&base, "alice", &project, "two").await;
    let two = w["id"].as_str().unwrap().to_owned();
    drive(&app, now).await;
    fake.boxes.lock().unwrap().retain(|_, b| b.metadata["arugula_workspace"] != two);
    drive(&app, now + 6_000).await;
    assert_eq!(show(&base, &project, &two).await["state"], "creating");
    drive(&app, now + 70_000).await;
    let w = show(&base, &project, &two).await;
    assert_eq!(w["state"], "failed");
    assert!(w["error"].as_str().unwrap().contains("didn't make the box"), "{w}");
    assert_eq!(fake.creates().len(), 2);

    // Refused: failed at once.
    fake.refuse_next.store(true, SeqCst);
    let (_, w) = make(&base, "alice", &project, "three").await;
    drive(&app, now).await;
    assert_eq!(show(&base, &project, w["id"].as_str().unwrap()).await["state"], "failed");
}

/// Failing closed: a project with no delegation, an enrollment the hub
/// says is taken, and a box that doesn't enroll in time each lose their
/// box.
#[tokio::test]
async fn enrolling_fails_closed() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let now = now_ms();
    drive(&app, now).await;
    let w = show(&base, &project, w["id"].as_str().unwrap()).await;
    assert_eq!(w["state"], "failed");
    assert_eq!(w["error"], enroll::NO_DELEGATION);
    assert!(fake.boxes().is_empty(), "its box was deleted");

    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "late").await;
    let id = w["id"].as_str().unwrap().to_owned();
    drive(&app, now).await;
    assert_eq!(fake.boxes().len(), 1);
    drive(&app, now + 120_000).await;
    assert_eq!(show(&base, &project, &id).await["state"], "enrolling", "within its window and the grace");
    drive(&app, now + 200_000).await;
    let w = show(&base, &project, &id).await;
    assert_eq!((w["state"].as_str(), w["error"].as_str()), (Some("failed"), Some(enroll::LATE)));
    assert!(fake.boxes().is_empty());
    assert!(hub.gone().contains(&id));

    // The hub says the box's sandbox id was enrolled already.
    let (_, w) = make(&base, "alice", &project, "taken").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let next = format!("sbx{}", fake.n.load(SeqCst));
    app.hub
        .hosted_enrollment(&enroll::EnrollAsk {
            project: project.clone(),
            owner: "team:t1".into(),
            workspace: "hw_other".into(),
            name: "other".into(),
            sandbox: Some(next),
        })
        .await
        .unwrap();
    drive(&app, now).await;
    let w = show(&base, &project, &id).await;
    assert_eq!((w["state"].as_str(), w["error"].as_str()), (Some("failed"), Some(enroll::TAKEN)));
    assert!(fake.boxes().is_empty());

    // The hub says someone tried the box's enrollment again after it was
    // used: the box goes.
    let (_, w) = make(&base, "alice", &project, "retried").await;
    let id = w["id"].as_str().unwrap().to_owned();
    drive(&app, now).await;
    assert_eq!(fake.boxes().len(), 1);
    hub.take(&id);
    drive(&app, now + 3_000).await;
    let w = show(&base, &project, &id).await;
    assert_eq!((w["state"].as_str(), w["error"].as_str()), (Some("failed"), Some(enroll::TAKEN)));
    assert!(fake.boxes().is_empty());
}

/// The hub's answer to recording a box's enrollment was lost: asking again
/// for the same box and workspace gets the one recorded, not `Taken`.
#[tokio::test]
async fn recording_an_enrollment_again_is_the_same_one() {
    let (hub, _fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let ask = enroll::EnrollAsk {
        project: project.clone(),
        owner: "team:t1".into(),
        workspace: id.clone(),
        name: "dev".into(),
        sandbox: Some("sbx0".into()),
    };
    // As though the worker's first ask reached the hub and its answer didn't.
    app.hub.hosted_enrollment(&ask).await.unwrap();
    drive(&app, now_ms()).await;
    assert_eq!(show(&base, &project, &id).await["state"], "enrolling");
}

/// Where boxes boot fresh (loam, testnet), the hub's one-time ticket and
/// control's URL go in the box's env, and nothing is asked by sandbox id.
#[tokio::test]
async fn ticket_mode_puts_a_ticket_in_the_env() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Ticket).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    drive(&app, now_ms()).await;
    let made = fake.creates();
    assert_eq!(made[0].env["ARUGULA_HOSTED_TICKET"], format!("ticket-{id}"));
    assert_eq!(made[0].env["ARUGULA_CONTROL_URL"], "https://control.example");
    assert_eq!(hub.enrollments().len(), 1);
    assert_eq!(hub.enrollments()[0].sandbox, None);
    assert_eq!(show(&base, &project, &id).await["state"], "enrolling");
}

/// A box that paused itself shows `paused`; opening it wakes it. Rebuild
/// deletes the box and makes another from the spec; delete takes every box
/// the workspace has, and its row leaves the list.
#[tokio::test]
async fn wake_rebuild_and_delete() {
    let (hub, fake, app, base, project) = serve(EnrollBy::Sandbox).await;
    hub.delegate(&project);
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let now = now_ms();
    drive(&app, now).await;
    hub.enroll(&id, "m1");
    drive(&app, now + 3_000).await;
    assert_eq!(show(&base, &project, &id).await["state"], "ready");

    fake.pause_all();
    drive(&app, now + 70_000).await;
    let w = show(&base, &project, &id).await;
    assert_eq!(w["asked"], "wake", "opening a paused box wakes it: {w}");
    drive(&app, now + 70_000).await;
    assert_eq!(show(&base, &project, &id).await["state"], "ready");
    assert_eq!(fake.boxes()[0].state, State::Running);

    // Rebuild: a new box, a new enrollment, the old machine gone.
    let path = format!("/api/projects/{project}/workspaces/{id}");
    let (status, w) = call(&base, "bob", "POST", &format!("{path}/rebuild"), None).await;
    assert_eq!((status, w["asked"].as_str()), (200, Some("rebuild")));
    drive(&app, now + 80_000).await;
    assert_eq!(fake.deleted.lock().unwrap().as_slice(), ["sbx0"]);
    assert_eq!(fake.boxes().iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["sbx1"]);
    assert_eq!(hub.gone(), std::slice::from_ref(&id));
    let w = show(&base, &project, &id).await;
    assert_eq!((w["state"].as_str(), w["machine"].as_str()), (Some("enrolling"), None));
    assert_eq!(hub.enrollments().last().unwrap().sandbox.as_deref(), Some("sbx1"));

    // Delete: every box it has, even one a lost create left.
    let mut stray = fake.boxes()[0].clone();
    stray.id = "stray".into();
    fake.boxes.lock().unwrap().insert("stray".into(), stray);
    let (status, w) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!((status, w["state"].as_str()), (200, Some("deleting")));
    drive(&app, now + 90_000).await;
    assert!(fake.boxes().is_empty());
    let (_, listed) = call(&base, "alice", "GET", &format!("/api/projects/{project}/workspaces"), None).await;
    assert_eq!(listed["workspaces"], json!([]));
    assert_eq!(call(&base, "alice", "GET", &path, None).await.0, 404);

    // Its usage (M188): running until the worker saw it paused, again from
    // its wake to its rebuild, and the new box until the delete.
    let ran: Vec<(u64, Option<u64>)> =
        app.db.hosted_usage(0, u64::MAX / 2).unwrap().iter().map(|i| (i.started_at, i.ended_at)).collect();
    assert_eq!(
        ran,
        [(now, Some(now + 70_000)), (now + 70_000, Some(now + 80_000)), (now + 80_000, Some(now + 90_000))]
    );
    assert_eq!(app.db.hosted_usage(0, u64::MAX / 2).unwrap()[0].class.map(|c| c.cpus), None, "never read");
    // This provider keeps no record of runs: each box is asked once.
    let later = now + 90_000 + super::usage::SETTLE_MS;
    assert_eq!(super::usage::reconcile(&app, later).await.unwrap(), 2);
    assert_eq!(super::usage::reconcile(&app, later + 1).await.unwrap(), 0);
}

/// A provider whose boxes all carry `arugula_test=1`, for the live run.
pub(crate) struct Tagged(pub(crate) compute::E2b);

impl Substrate for Tagged {
    fn create<'a>(&'a self, b: &'a Create) -> BoxFuture<'a, compute::Result<Created>> {
        Box::pin(async move {
            let mut b = b.clone();
            b.metadata.insert("arugula_test".into(), "1".into());
            self.0.create(&b).await
        })
    }
    fn resume<'a>(&'a self, id: &'a str, t: Duration) -> BoxFuture<'a, compute::Result<Resumed>> {
        self.0.resume(id, t)
    }
    fn pause<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<Paused>> {
        self.0.pause(id)
    }
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<()>> {
        self.0.delete(id)
    }
    fn status<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<Option<Sandbox>>> {
        self.0.status(id)
    }
    fn list<'a>(&'a self, m: &'a BTreeMap<String, String>) -> BoxFuture<'a, compute::Result<Vec<Sandbox>>> {
        self.0.list(m)
    }
    fn hold<'a>(&'a self, id: &'a str, t: Duration) -> BoxFuture<'a, compute::Result<()>> {
        self.0.hold(id, t)
    }
}

/// The lifecycle against a real provider (E2B's cloud or loam's
/// E2B-compatible API), in ticket mode, with the test hub standing in
/// for M178's: make a workspace's box, pause it, open it (waking it), and
/// delete it, leaving nothing. Ignored unless asked for:
///
/// ```sh
/// ARUGULA_TEST_E2B_URL=https://api.e2b.loam.inevitable.fyi ARUGULA_TEST_E2B_KEY=... \
///   cargo test -p arugula-platform --lib hosted::tests::live -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "makes a real box: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn live_lifecycle() {
    let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
        panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
    };
    let template = std::env::var("ARUGULA_TEST_E2B_TEMPLATE").unwrap_or_else(|_| "base".into());
    let (hub, _fake, app, base, project) = serve(EnrollBy::Ticket).await;
    let live = Arc::new(Tagged(compute::E2b::new(&url, key).unwrap()));
    // `serve`'s platform with the real provider in place of the fake.
    let app = {
        let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
        let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
        p.compute = Some(live.clone());
        p.hosted = Config { template: Some(template.clone()), ..app.hosted.clone() };
        Arc::new(p)
    };
    let project_id = app.db.team_project("t1", "t1", 0).unwrap();
    let _ = (base, project);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::cloud::tests::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    hub.delegate(&project_id);

    let (status, w) = make(&base, "alice", &project_id, "live").await;
    assert_eq!(status, 201, "{w}");
    let id = w["id"].as_str().unwrap().to_owned();
    let mine: BTreeMap<String, String> = [("arugula_workspace".to_owned(), id.clone())].into();

    let outcome = AssertUnwindSafe(async {
        let until = |state: &'static str, secs: u64| {
            let (app, base, project_id, id) = (app.clone(), base.clone(), project_id.clone(), id.clone());
            async move {
                let started = std::time::Instant::now();
                loop {
                    worker::tick_at(&app, now_ms()).await.unwrap();
                    let w = show(&base, &project_id, &id).await;
                    if w["state"] == state {
                        println!("{state} after {} ms", started.elapsed().as_millis());
                        return w;
                    }
                    assert!(started.elapsed() < Duration::from_secs(secs), "never {state}: {w}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        };
        until("enrolling", 120).await;
        let boxes = live.list(&mine).await.unwrap();
        assert_eq!(boxes.len(), 1, "one box, found by its metadata");
        assert_eq!(boxes[0].metadata["arugula_project"], project_id);
        assert_eq!(boxes[0].metadata["arugula_template"], template);
        hub.enroll(&id, "live-machine");
        until("ready", 30).await;

        assert!(crate::hosted::ask(&app, &id, crate::hosted::Ask::Pause).unwrap());
        until("paused", 120).await;
        assert_eq!(live.status(&boxes[0].id).await.unwrap().map(|s| s.state), Some(State::Paused));
        // Opening it asks for the wake.
        assert_eq!(show(&base, &project_id, &id).await["asked"], "wake");
        until("ready", 120).await;
        assert_eq!(live.status(&boxes[0].id).await.unwrap().map(|s| s.state), Some(State::Running));

        let (status, _) =
            call(&base, "alice", "DELETE", &format!("/api/projects/{project_id}/workspaces/{id}"), None).await;
        assert_eq!(status, 200);
        for _ in 0..20 {
            worker::tick_at(&app, now_ms()).await.unwrap();
        }
        assert!(live.list(&mine).await.unwrap().is_empty(), "deleted");
        let (_, listed) = call(&base, "alice", "GET", &format!("/api/projects/{project_id}/workspaces"), None).await;
        assert_eq!(listed["workspaces"], json!([]));
    })
    .catch_unwind()
    .await;
    // Whatever happened, nothing's left.
    for b in live.list(&mine).await.unwrap_or_default() {
        let _ = live.delete(&b.id).await;
    }
    assert!(live.list(&mine).await.unwrap().is_empty(), "a box was left behind");
    if let Err(p) = outcome {
        std::panic::resume_unwind(p);
    }
}
