//! Feature crates (`arugula-platform-<area>`, #314): what each adds to the
//! platform, registered before it starts. Both binaries register every
//! one (`arugula-platform-all`), so the platform in the hub's process and
//! the `arugula-platform` binary are the same platform. The base calls
//! these hooks where it used to name the feature; a feature that isn't
//! registered adds nothing. A feature keeps what it holds in memory in
//! [`Slots`] on the [`Platform`].

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, Mutex},
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::{
    chat::{
        AgentMessages, AgentMessagesQuery, AgentRooms, ChatFound, ChatMessage, ChatPost, ChatPosted, ChatSearchQuery,
    },
    work::{
        ArtifactGiven, DeployWork, PushAsk, PushGiven, Reported, SecretsAsk, SecretsGiven, SessionIssuesAsk,
        SessionIssuesGiven, SessionReport,
    },
};
use axum::{Router, http::StatusCode, routing::MethodRouter};
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    Platform,
    auth::Machine,
    db::{Db, mirror::ItemRow, runs::Run, sessions::Session},
    forge::send::{How, Outcome},
    jobs::Worker,
    triggers::events::Seen,
    work::HostedMachine,
};

/// What a feature crate adds.
pub trait Feature: Send + Sync + 'static {
    /// Its routes, beside the base's ([`crate::router`], [`crate::paths`]).
    fn routes(&self, r: Routes) -> Routes {
        r
    }
    /// Its background work, once the platform starts (in the hub's process
    /// or as its own binary).
    fn start(&self, _p: &Arc<Platform>) {}
    /// Its kinds of job, for the one job worker (`work::start`).
    fn jobs(&self, _p: &Arc<Platform>, w: Worker) -> Worker {
        w
    }
}

static ALL: Mutex<Vec<&'static dyn Feature>> = Mutex::new(Vec::new());

/// Puts a feature in this platform: before it's made, by the binary (or a
/// test).
pub fn register(f: Box<dyn Feature>) {
    ALL.lock().unwrap().push(Box::leak(f));
}

pub(crate) fn all() -> Vec<&'static dyn Feature> {
    ALL.lock().unwrap().clone()
}

/// Every feature's background work ([`Feature::start`]).
pub fn start(p: &Arc<Platform>) {
    for f in all() {
        f.start(p);
    }
}

/// Every feature's kinds of job ([`Feature::jobs`]), with the worker's own.
pub fn jobs(p: &Arc<Platform>, w: Worker) -> Worker {
    all().into_iter().fold(w, |w, f| f.jobs(p, w))
}

/// Deploys (`arugula-platform-deploys`), as the rest of the platform asks
/// them: the runs engine for a deploy's run, a deploy box for its secrets
/// and artifact, agents with no machine for a plan. Without the crate, a
/// run is no deploy's and every ask is refused.
pub trait Deploys: Send + Sync + 'static {
    /// A deploy's run whose deploy's job gave up: its owners hear.
    fn page_unpaged<'a>(&'a self, _app: &'a Platform, _run: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
    }
    /// What a deploy's run carries to the box that applies it.
    fn deploy_work(&self, _db: &Db, _run: &str) -> anyhow::Result<Option<DeployWork>> {
        Ok(None)
    }
    /// A deploy's run moved: its deploy moves too, and the box may be told
    /// otherwise.
    fn reported<'a>(&'a self, _app: &'a Platform, _run: &'a Run) -> BoxFuture<'a, anyhow::Result<Option<Reported>>> {
        Box::pin(async { Ok(None) })
    }
    /// A deploy box's secrets (`PlatformPort::hosted_secrets`).
    fn secrets<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<SecretsGiven>> {
        Box::pin(async { Ok(SecretsGiven::Refused { why: "no deploys here".into() }) })
    }
    /// A deploy box's artifact (`PlatformPort::hosted_artifact`).
    fn artifact<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<ArtifactGiven>> {
        Box::pin(async { Ok(ArtifactGiven::Refused { why: "no deploys here".into() }) })
    }
    /// A machine's agent plans a deploy (`POST /api/daemon/projects/{project}/deploys`).
    fn agent_plan(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _project: String,
        _plan: Value,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
    /// A machine's agent asks for a deploy's approval.
    fn agent_request(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _id: String,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
    /// A deploy, as a machine's agent may read it.
    fn agent_get(&self, _app: Arc<Platform>, _me: Machine, _id: String) -> BoxFuture<'static, Result<Value, ApiError>> {
        Box::pin(async { Err(no_deploys()) })
    }
}

fn no_deploys() -> ApiError {
    err(StatusCode::NOT_FOUND, "no deploys here")
}

struct NoDeploys;

impl Deploys for NoDeploys {}

static DEPLOYS: std::sync::OnceLock<Box<dyn Deploys>> = std::sync::OnceLock::new();

/// Puts deploys in this platform (`arugula-platform-deploys`'s `register`).
pub fn install_deploys(d: Box<dyn Deploys>) {
    let _ = DEPLOYS.set(d);
}

/// Deploys, as registered; refusing everything when they aren't.
pub fn deploys() -> &'static dyn Deploys {
    DEPLOYS.get().map_or(&NoDeploys, |d| d.as_ref())
}

/// Chat and agent sessions (`arugula-platform-chat`), as the rest of the
/// platform asks them: the hub's port for a session's PR and a hosted box's
/// session calls, the forge mirror and GitHub's webhook for what a
/// session hears, hosted workspaces for a rebuilt box's sessions, boxes for
/// a review, grants for the notice a room gets, and agents with no machine
/// for their threads. Chat's core (the live bus, whether chat is on, names
/// and mentions) is the base's ([`crate::chat`]). Without the crate there's
/// no session and no thread: each ask is refused, and what a session would
/// hear is nobody's.
pub trait Chat: Send + Sync + 'static {
    /// Whether `draft` is a session's PR (M259), which the hub's
    /// `send_draft` callback sends through [`Chat::send_pr`], not a forge
    /// draft.
    fn is_pr(&self, _draft: &str) -> bool {
        false
    }
    /// Sends a session's PR, as its asker.
    fn send_pr<'a>(
        &'a self,
        _app: &'a Platform,
        _account: &'a str,
        _draft: &'a str,
        _how: How,
    ) -> BoxFuture<'a, Outcome> {
        Box::pin(async { Outcome { refused: Some("no sessions here".into()), ..Default::default() } })
    }
    /// What a hosted box says of one of its sessions
    /// (`PlatformPort::hosted_session_report`).
    fn session_report<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _report: &'a SessionReport,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        Box::pin(async { Ok(Reported::Refused { why: "no sessions here".into(), now: None, next: None }) })
    }
    /// A session's push token (`PlatformPort::hosted_session_push`).
    fn session_push<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a PushAsk,
    ) -> BoxFuture<'a, anyhow::Result<PushGiven>> {
        Box::pin(async { Ok(PushGiven::Refused { why: "no sessions here".into() }) })
    }
    /// A session's issues call (`PlatformPort::hosted_session_issues`).
    fn session_issues<'a>(
        &'a self,
        _app: &'a Platform,
        _m: &'a HostedMachine,
        _ask: &'a SessionIssuesAsk,
    ) -> BoxFuture<'a, anyhow::Result<SessionIssuesGiven>> {
        Box::pin(async { Ok(SessionIssuesGiven::Refused { why: "no sessions here".into() }) })
    }
    /// The forge mirror wrote new items of `repo`: a session's PR among them
    /// says where it came from (M259).
    fn pr_noted(&self, _app: &Platform, _repo: u64) {}
    /// The forge mirror saw `items` of `repo` change: a session's PR merged
    /// ends the session (M265).
    fn merged_seen(&self, _app: &Platform, _repo: u64, _items: &[&ItemRow]) {}
    /// A session's prompt on its workspace's new box, and whether its
    /// conversation was lost with the old one (Arugula #209).
    fn after_rebuild<'a>(&'a self, _app: &'a Platform, _s: &'a Session) -> BoxFuture<'a, anyhow::Result<Rebuilt>> {
        Box::pin(async { anyhow::bail!("no sessions here") })
    }
    /// Sessions `ids` changed: their cards and their asks hear it.
    fn told(&self, _app: &Platform, _ids: &[String]) -> anyhow::Result<()> {
        Ok(())
    }
    /// A check suite finished: on a session's branch, its session hears.
    fn suite_done(&self, _app: &Platform, _event: &str, _v: &Value, _now: u64) -> anyhow::Result<()> {
        Ok(())
    }
    /// GitHub's event `seen`, if it's on a session's branch: what came of it.
    fn route_feedback(&self, _app: &Platform, _seen: &Seen, _v: &Value, _now: u64) -> anyhow::Result<Routed> {
        Ok(Routed::Elsewhere)
    }
    /// A box's review (`boxes::post`), into the thread on `target` in
    /// `project` for `author`: the room and the message's number.
    fn box_post<'a>(
        &'a self,
        _app: &'a Platform,
        _author: &'a str,
        _project: &'a str,
        _target: &'a str,
        _via: &'a str,
        _text: String,
    ) -> BoxFuture<'a, Result<(String, i64), ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// Before a grant of `power` over `scope` is given (#122): the rooms
    /// it lets agents into, and who reaches them now. `None` when it lets
    /// agents into none.
    fn grant_notice<'a>(
        &'a self,
        _app: &'a Platform,
        _power: &'a str,
        _scope: (&'a str, &'a str),
    ) -> BoxFuture<'a, Option<GrantNotice>> {
        Box::pin(async { None })
    }
    /// After it was given: those rooms say whose agents reach them now.
    fn announce<'a>(&'a self, _app: &'a Platform, _notice: GrantNotice) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    /// A machine's agents' rooms (`AGENT_ROOMS`), for agents with no machine
    /// too.
    fn agent_rooms(&self, _app: Arc<Platform>, _me: Machine) -> BoxFuture<'static, Result<AgentRooms, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// A room's messages, as a machine's agents read them.
    fn agent_messages(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _room: String,
        _q: AgentMessagesQuery,
    ) -> BoxFuture<'static, Result<AgentMessages, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// A machine's agents post in a room.
    fn agent_post(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _room: String,
        _b: ChatPost,
    ) -> BoxFuture<'static, Result<ChatPosted, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// A machine's agents react to message `seq` (`on`), or take it back.
    fn agent_react(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _room: String,
        _seq: i64,
        _emoji: String,
        _on: bool,
    ) -> BoxFuture<'static, Result<ChatMessage, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// A machine's agents pin message `seq` (`on`), or unpin it.
    fn agent_pin(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _room: String,
        _seq: i64,
        _on: bool,
    ) -> BoxFuture<'static, Result<ChatMessage, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
    /// A machine's agents search the rooms they reach.
    fn agent_search(
        &self,
        _app: Arc<Platform>,
        _me: Machine,
        _q: ChatSearchQuery,
    ) -> BoxFuture<'static, Result<ChatFound, ApiError>> {
        Box::pin(async { Err(no_chat()) })
    }
}

/// What came of an event on a session's branch ([`Chat::route_feedback`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Not a session's: the rules'.
    Elsewhere,
    /// A session's, from outside the repo: nothing comes of it.
    Outsider { session: String },
    /// Gathered into a batch; `new: false` when it was there already (a
    /// redelivery).
    Gathered { session: String, new: bool },
    /// Its batch was sent already: kept out.
    Late { session: String },
    /// Its session ended: dropped, and the thread told (once).
    Ended { session: String },
}

/// A session's prompt on its workspace's new box ([`Chat::after_rebuild`]),
/// and whether its conversation was lost: the old box archived no
/// transcript of it (Arugula #209), which its card says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    pub text: String,
    pub lost: bool,
}

/// What [`Chat::grant_notice`] found before a grant, for [`Chat::announce`]
/// after it: chat's own.
pub struct GrantNotice(pub Box<dyn Any + Send + Sync>);

fn no_chat() -> ApiError {
    err(StatusCode::NOT_FOUND, "no chat here")
}

struct NoChat;

impl Chat for NoChat {}

static CHAT: std::sync::OnceLock<Box<dyn Chat>> = std::sync::OnceLock::new();

/// Puts chat and its sessions in this platform (`arugula-platform-chat`'s
/// `register`).
pub fn install_chat(c: Box<dyn Chat>) {
    let _ = CHAT.set(c);
}

/// Chat and its sessions, as registered; refusing everything when they
/// aren't.
pub fn chat() -> &'static dyn Chat {
    CHAT.get().map_or(&NoChat, |c| c.as_ref())
}

/// A router that notes its paths too: routes.toml's test reads them.
#[derive(Default)]
pub struct Routes {
    pub(crate) router: Router<Arc<Platform>>,
    pub(crate) paths: Vec<&'static str>,
}

impl Routes {
    pub fn route(mut self, path: &'static str, methods: MethodRouter<Arc<Platform>>) -> Self {
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}

/// What feature crates keep on the platform: one value of each type, put
/// while the platform is made and read from then on.
#[derive(Default)]
pub struct Slots(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Slots {
    pub fn put<T: Any + Send + Sync>(&mut self, v: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(v));
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }
}
