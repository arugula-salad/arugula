//! The seam over the private network, once the platform is its own process
//! (M140): [`HttpHub`] is `HubPort` over HTTP and [`serve_hub`] answers it
//! (the hub mounts that on its internal listener); [`HttpPlatform`] is
//! `PlatformPort` over HTTP and [`serve_platform`] answers it (the platform
//! binary's). Both directions live here, beside the traits, so a method
//! and its wire are one change. And [`vouched`] is the platform's check of
//! the principal header the hub's gateway forwards every call with.
//!
//! Each method is one `POST /internal/<hub|platform>/v1/<method>`, its
//! arguments a JSON object, its answer the method's JSON (200), or
//! `{"error": ...}` (500, or 404 for a method this side doesn't have yet:
//! the side that's called ships a method a release before the caller uses
//! it). Every call is signed (`arugula_control_common::internal`): the key
//! never crosses the network.
//!
//! The types keep `arugula-control-wire`'s two rules: the bytes don't
//! change, and nothing is `deny_unknown_fields`.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Context as _;
use arugula_control_common::{
    Principal, err,
    internal::{self, Call, Callee, Nonces},
    principal::{self, NotVouched, PrincipalKey, Vouch},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tracing::Instrument as _;

use arugula_control_wire::{
    forge::{DaemonOp, OpAnswer, Poke, Standing},
    hosted::{EnrollAsk, EnrollGiven, Enrolled, WakeAnswer},
};

use crate::{
    Deleted, PlatformPort,
    auth::Asked,
    forge::{
        GithubUser, TokenAnswer,
        send::{Outcome, UserToken},
    },
    hub::{Account, ChangePage, Daemon, Flags, HubPort, HubResult, Meter, PushOptions, Pushed, Resolved, Team},
};

/// Where the hub's internal API is mounted.
pub const HUB_PREFIX: &str = "/internal/hub/v1";
/// Where the platform's is.
pub const PLATFORM_PREFIX: &str = "/internal/platform/v1";

/// The largest internal call's arguments: a page of ids, a push message.
const MAX_CALL: usize = 4 * 1024 * 1024;

/// How long one internal call may take: a push waits for push services,
/// and a roster change for Stripe, each of which gives up sooner.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// One side's way to the other: its internal API's base URL (no trailing
/// slash), the shared key, and an HTTP client of its own.
#[derive(Clone)]
struct Client {
    base: String,
    key: PrincipalKey,
    http: reqwest::Client,
    to: Callee,
    prefix: &'static str,
}

impl Client {
    fn new(base: &str, key: PrincipalKey, to: Callee, prefix: &'static str) -> anyhow::Result<Self> {
        let http = arugula_control_common::roots::build(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(CALL_TIMEOUT)
                .tcp_nodelay(true)
                .no_proxy()
        })?;
        Ok(Client { base: base.trim_end_matches('/').to_owned(), key, http, to, prefix })
    }

    async fn call<A: Serialize, T: DeserializeOwned>(&self, method: &str, args: A) -> anyhow::Result<T> {
        let path = format!("{}/{method}", self.prefix);
        let body = serde_json::to_vec(&args)?;
        let sig = internal::sign(&self.key, self.to, "POST", &path, &body, arugula_e2e::now_ms());
        let res = self
            .http
            .post(format!("{}{path}", self.base))
            .header(internal::HEADER, sig)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .with_context(|| format!("calling {} {method}", self.to_name()))?;
        let status = res.status();
        let bytes = res.bytes().await?;
        if !status.is_success() {
            let error = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
            anyhow::bail!("{} {method}: {status}: {error}", self.to_name());
        }
        serde_json::from_slice(&bytes).with_context(|| format!("{} {method}'s answer", self.to_name()))
    }

    fn to_name(&self) -> &'static str {
        match self.to {
            Callee::Hub => "the hub's",
            Callee::Platform => "the platform's",
        }
    }
}

// ---------------------------------------------------------------- arguments

#[derive(Serialize, Deserialize)]
struct ResolveArgs {
    principal: Principal,
}

#[derive(Serialize, Deserialize)]
struct IdsArgs {
    ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct AccountArgs {
    account: String,
}

#[derive(Serialize, Deserialize)]
struct WordArgs {
    word: String,
}

#[derive(Serialize, Deserialize)]
struct ReachArgs {
    account: String,
    machines: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct PushArgs {
    account: String,
    message: Value,
    #[serde(default)]
    options: PushOptions,
}

#[derive(Serialize, Deserialize)]
struct OwnersArgs {
    owners: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct MetersArgs {
    accounts: Vec<String>,
    since: u64,
    until: u64,
}

#[derive(Serialize, Deserialize)]
struct ChangesArgs {
    after: u64,
    limit: u32,
}

#[derive(Serialize, Deserialize)]
struct DriveArgs {
    daemon: String,
    account: String,
}

#[derive(Serialize, Deserialize)]
struct TeamArgs {
    team: String,
}

#[derive(Serialize, Deserialize)]
struct BlockersArgs {
    account: String,
    start: bool,
}

#[derive(Serialize, Deserialize)]
struct DeletedArgs {
    deleted: Deleted,
}

#[derive(Serialize, Deserialize, Default)]
struct NoArgs {}

#[derive(Serialize, Deserialize)]
struct PokesArgs {
    pokes: Vec<Poke>,
}

#[derive(Serialize, Deserialize)]
struct StandingArgs {
    account: String,
    github: Option<GithubUser>,
    repos: Vec<String>,
    fresh: bool,
}

#[derive(Serialize, Deserialize)]
struct TokenArgs {
    account: String,
    github: Option<GithubUser>,
    repo: String,
}

#[derive(Serialize, Deserialize)]
struct SendAuthArgs {
    account: String,
    draft: String,
    back: String,
}

#[derive(Serialize, Deserialize)]
struct WorkspacesArgs {
    workspaces: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct DelegationArgs {
    project: String,
    owner: String,
}

#[derive(Serialize, Deserialize)]
struct OpArgs {
    daemon: String,
    op: DaemonOp,
}

/// The user token crosses inside the signed call's body, never logged.
#[derive(Serialize, Deserialize)]
struct SendArgs {
    account: String,
    draft: String,
    #[serde(default)]
    token: Option<UserToken>,
}

/// `PlatformPort::hosted_wake` (M182).
#[derive(Serialize, Deserialize)]
struct WakeArgs {
    project: String,
    workspace: String,
}

/// A hosted box's turn (M185): its workspace and machine, as the hub's
/// hosted certificate names them, and the agent or the turn.
#[derive(Serialize, Deserialize)]
struct TurnArgs {
    workspace: String,
    machine: String,
    #[serde(default)]
    agent: String,
    #[serde(default)]
    turn: String,
}

fn ids(v: &[String]) -> IdsArgs {
    IdsArgs { ids: v.to_vec() }
}

// ---------------------------------------------------------------- HubPort over HTTP

/// `HubPort` over the private network: the out-of-process platform's view
/// of the hub, at the hub's internal listener
/// (`http://illogical-control.internal:7691` on Fly).
#[derive(Clone)]
pub struct HttpHub(Client);

impl HttpHub {
    pub fn new(base: &str, key: PrincipalKey) -> anyhow::Result<Self> {
        Ok(HttpHub(Client::new(base, key, Callee::Hub, HUB_PREFIX)?))
    }
}

impl HubPort for HttpHub {
    fn resolve<'a>(&'a self, principal: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        Box::pin(self.0.call("resolve", ResolveArgs { principal: principal.clone() }))
    }

    fn accounts<'a>(&'a self, ids_: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        Box::pin(self.0.call("accounts", ids(ids_)))
    }

    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        Box::pin(self.0.call("accounts_called", WordArgs { word: word.to_owned() }))
    }

    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(self.0.call("teams_of", AccountArgs { account: account.to_owned() }))
    }

    fn teams<'a>(&'a self, ids_: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        Box::pin(self.0.call("teams", ids(ids_)))
    }

    fn daemons<'a>(&'a self, ids_: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        Box::pin(self.0.call("daemons", ids(ids_)))
    }

    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(self.0.call("machines_for", AccountArgs { account: account.to_owned() }))
    }

    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(self.0.call("may_reach", ReachArgs { account: account.to_owned(), machines: machines.to_vec() }))
    }

    fn push<'a>(&'a self, account: &'a str, message: &'a Value) -> BoxFuture<'a, HubResult<Pushed>> {
        self.push_with(account, message, PushOptions::default())
    }

    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>> {
        Box::pin(self.0.call("push_with", PushArgs { account: account.to_owned(), message: message.clone(), options }))
    }

    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
        Box::pin(self.0.call("labs_flags", OwnersArgs { owners: owners.to_vec() }))
    }

    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        Box::pin(self.0.call("meters", MetersArgs { accounts: accounts.to_vec(), since, until }))
    }

    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        Box::pin(self.0.call("metered_accounts", NoArgs {}))
    }

    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        Box::pin(self.0.call("changes", ChangesArgs { after, limit }))
    }

    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(self.0.call("personal_project", AccountArgs { account: account.to_owned() }))
    }

    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        Box::pin(self.0.call("can_drive_cloud", DriveArgs { daemon: daemon.to_owned(), account: account.to_owned() }))
    }

    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(self.0.call("team_project", TeamArgs { team: team.to_owned() }))
    }

    fn forge_pokes<'a>(&'a self, pokes: &'a [Poke]) -> BoxFuture<'a, HubResult<u32>> {
        Box::pin(self.0.call("forge_pokes", PokesArgs { pokes: pokes.to_vec() }))
    }

    fn github_user<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<GithubUser>>> {
        Box::pin(self.0.call("github_user", AccountArgs { account: account.to_owned() }))
    }

    fn send_authorization<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        back: &'a str,
    ) -> BoxFuture<'a, HubResult<String>> {
        let a = SendAuthArgs { account: account.to_owned(), draft: draft.to_owned(), back: back.to_owned() };
        Box::pin(self.0.call("send_authorization", a))
    }

    fn daemon_op<'a>(&'a self, daemon: &'a str, op: &'a DaemonOp) -> BoxFuture<'a, HubResult<OpAnswer>> {
        Box::pin(self.0.call("daemon_op", OpArgs { daemon: daemon.to_owned(), op: op.clone() }))
    }

    fn hosted_enrollment<'a>(&'a self, ask: &'a EnrollAsk) -> BoxFuture<'a, HubResult<EnrollGiven>> {
        Box::pin(self.0.call("hosted_enrollment", ask.clone()))
    }

    fn hosted_enrolled<'a>(&'a self, workspaces: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Enrolled>>> {
        Box::pin(self.0.call("hosted_enrolled", WorkspacesArgs { workspaces: workspaces.to_vec() }))
    }

    fn hosted_gone<'a>(&'a self, workspaces: &'a [String]) -> BoxFuture<'a, HubResult<()>> {
        Box::pin(self.0.call("hosted_gone", WorkspacesArgs { workspaces: workspaces.to_vec() }))
    }

    fn live_delegation<'a>(&'a self, project: &'a str, owner: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(
            self.0.call("live_delegation", DelegationArgs { project: project.to_owned(), owner: owner.to_owned() }),
        )
    }

    fn activity<'a>(&'a self, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<crate::hub::MachineActivity>>> {
        Box::pin(self.0.call("activity", IdsArgs { ids: machines.to_vec() }))
    }
}

/// What one side's internal API answers with: the port it serves, and
/// what checks its callers.
struct Serving<P: ?Sized> {
    port: Arc<P>,
    key: PrincipalKey,
    nonces: Nonces,
    to: Callee,
}

/// The hub's internal API: every `HubPort` method of `hub`, for
/// [`HttpHub`]. The hub mounts it on a listener of its own that only the
/// private network reaches.
pub fn serve_hub(hub: Arc<dyn HubPort>, key: PrincipalKey) -> Router {
    let s = Arc::new(Serving { port: hub, key, nonces: Nonces::default(), to: Callee::Hub });
    Router::new()
        .route(&format!("{HUB_PREFIX}/{{method}}"), post(hub_call))
        .layer(axum::middleware::from_fn_with_state(s.clone(), signed::<dyn HubPort>))
        .with_state(s)
}

fn args<A: DeserializeOwned>(body: &[u8]) -> anyhow::Result<A> {
    serde_json::from_slice(body).context("the call's arguments")
}

fn answer<T: Serialize>(t: T) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(t)?)
}

fn not_here(method: &str) -> Response {
    err(StatusCode::NOT_FOUND, &format!("no internal method {method} here")).into_response()
}

fn answered(method: &str, r: anyhow::Result<Value>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(method, error = format!("{e:#}"), "an internal call failed");
            err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")).into_response()
        }
    }
}

async fn hub_call(State(s): State<Arc<Serving<dyn HubPort>>>, Path(method): Path<String>, body: Bytes) -> Response {
    let hub = &*s.port;
    let r = async {
        Ok(Some(match method.as_str() {
            "resolve" => answer(hub.resolve(&args::<ResolveArgs>(&body)?.principal).await?)?,
            "accounts" => answer(hub.accounts(&args::<IdsArgs>(&body)?.ids).await?)?,
            "accounts_called" => answer(hub.accounts_called(&args::<WordArgs>(&body)?.word).await?)?,
            "teams_of" => answer(hub.teams_of(&args::<AccountArgs>(&body)?.account).await?)?,
            "teams" => answer(hub.teams(&args::<IdsArgs>(&body)?.ids).await?)?,
            "daemons" => answer(hub.daemons(&args::<IdsArgs>(&body)?.ids).await?)?,
            "machines_for" => answer(hub.machines_for(&args::<AccountArgs>(&body)?.account).await?)?,
            "may_reach" => {
                let a: ReachArgs = args(&body)?;
                answer(hub.may_reach(&a.account, &a.machines).await?)?
            }
            "push_with" => {
                let a: PushArgs = args(&body)?;
                answer(hub.push_with(&a.account, &a.message, a.options).await?)?
            }
            "labs_flags" => answer(hub.labs_flags(&args::<OwnersArgs>(&body)?.owners).await?)?,
            "meters" => {
                let a: MetersArgs = args(&body)?;
                answer(hub.meters(&a.accounts, a.since, a.until).await?)?
            }
            "metered_accounts" => answer(hub.metered_accounts().await?)?,
            "changes" => {
                let a: ChangesArgs = args(&body)?;
                answer(hub.changes(a.after, a.limit).await?)?
            }
            "personal_project" => answer(hub.personal_project(&args::<AccountArgs>(&body)?.account).await?)?,
            "can_drive_cloud" => {
                let a: DriveArgs = args(&body)?;
                answer(hub.can_drive_cloud(&a.daemon, &a.account).await?)?
            }
            "team_project" => answer(hub.team_project(&args::<TeamArgs>(&body)?.team).await?)?,
            "forge_pokes" => answer(hub.forge_pokes(&args::<PokesArgs>(&body)?.pokes).await?)?,
            "github_user" => answer(hub.github_user(&args::<AccountArgs>(&body)?.account).await?)?,
            "send_authorization" => {
                let a: SendAuthArgs = args(&body)?;
                answer(hub.send_authorization(&a.account, &a.draft, &a.back).await?)?
            }
            "daemon_op" => {
                let a: OpArgs = args(&body)?;
                answer(hub.daemon_op(&a.daemon, &a.op).await?)?
            }
            "hosted_enrollment" => answer(hub.hosted_enrollment(&args::<EnrollAsk>(&body)?).await?)?,
            "hosted_enrolled" => answer(hub.hosted_enrolled(&args::<WorkspacesArgs>(&body)?.workspaces).await?)?,
            "hosted_gone" => answer(hub.hosted_gone(&args::<WorkspacesArgs>(&body)?.workspaces).await?)?,
            "activity" => answer(hub.activity(&args::<IdsArgs>(&body)?.ids).await?)?,
            "live_delegation" => {
                let a: DelegationArgs = args(&body)?;
                answer(hub.live_delegation(&a.project, &a.owner).await?)?
            }
            _ => return Ok(None),
        }))
    }
    .await;
    match r {
        Ok(Some(v)) => answered(&method, Ok(v)),
        Ok(None) => not_here(&method),
        Err(e) => answered(&method, Err(e)),
    }
}

// ---------------------------------------------------------------- PlatformPort over HTTP

/// `PlatformPort` over the private network: the hub's view of the
/// out-of-process platform (`http://arugula-platform.internal:7700` on
/// Fly, or through Flycast from M145, `http://arugula-platform.flycast:7700`:
/// any of its machines answers, and nothing here depends on which, or on
/// the caller's address).
#[derive(Clone)]
pub struct HttpPlatform(Client);

impl HttpPlatform {
    pub fn new(base: &str, key: PrincipalKey) -> anyhow::Result<Self> {
        Ok(HttpPlatform(Client::new(base, key, Callee::Platform, PLATFORM_PREFIX)?))
    }
}

impl PlatformPort for HttpPlatform {
    fn delete_blockers<'a>(&'a self, account: &'a str, start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(self.0.call("delete_blockers", BlockersArgs { account: account.to_owned(), start }))
    }

    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let _: Value = self.0.call("account_deleted", DeletedArgs { deleted: deleted.clone() }).await?;
            Ok(())
        })
    }

    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(self.0.call("active_plans", OwnersArgs { owners: owners.to_vec() }))
    }

    fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(self.0.call("chat_on", AccountArgs { account: account.to_owned() }))
    }

    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let _: Value = self.0.call("roster_changed", TeamArgs { team: team.to_owned() }).await?;
            Ok(())
        })
    }

    fn forge_standing<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repos: &'a [String],
        fresh: bool,
    ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>> {
        let a = StandingArgs { account: account.to_owned(), github: github.cloned(), repos: repos.to_vec(), fresh };
        Box::pin(self.0.call("forge_standing", a))
    }

    fn github_token<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repo: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>> {
        let a = TokenArgs { account: account.to_owned(), github: github.cloned(), repo: repo.to_owned() };
        Box::pin(self.0.call("github_token", a))
    }

    fn send_draft<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        token: Option<UserToken>,
    ) -> BoxFuture<'a, anyhow::Result<Outcome>> {
        let a = SendArgs { account: account.to_owned(), draft: draft.to_owned(), token };
        Box::pin(self.0.call("send_draft", a))
    }

    fn hosted_wake<'a>(&'a self, project: &'a str, workspace: &'a str) -> BoxFuture<'a, anyhow::Result<WakeAnswer>> {
        let a = WakeArgs { project: project.to_owned(), workspace: workspace.to_owned() };
        Box::pin(self.0.call("hosted_wake", a))
    }

    fn hosted_turn<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        agent: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<crate::port::TurnAnswer>> {
        let a = TurnArgs {
            workspace: workspace.to_owned(),
            machine: machine.to_owned(),
            agent: agent.to_owned(),
            turn: String::new(),
        };
        Box::pin(self.0.call("hosted_turn", a))
    }

    fn hosted_turn_end<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        turn: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        let a = TurnArgs {
            workspace: workspace.to_owned(),
            machine: machine.to_owned(),
            agent: String::new(),
            turn: turn.to_owned(),
        };
        Box::pin(self.0.call("hosted_turn_end", a))
    }
}

/// The platform's internal API: every `PlatformPort` method of `port`,
/// for [`HttpPlatform`].
pub fn serve_platform(port: Arc<dyn PlatformPort>, key: PrincipalKey) -> Router {
    let s = Arc::new(Serving { port, key, nonces: Nonces::default(), to: Callee::Platform });
    Router::new()
        .route(&format!("{PLATFORM_PREFIX}/{{method}}"), post(platform_call))
        .layer(axum::middleware::from_fn_with_state(s.clone(), signed::<dyn PlatformPort>))
        .with_state(s)
}

async fn platform_call(
    State(s): State<Arc<Serving<dyn PlatformPort>>>,
    Path(method): Path<String>,
    body: Bytes,
) -> Response {
    let p = &*s.port;
    let r = async {
        Ok(Some(match method.as_str() {
            "delete_blockers" => {
                let a: BlockersArgs = args(&body)?;
                answer(p.delete_blockers(&a.account, a.start).await?)?
            }
            "account_deleted" => {
                p.account_deleted(&args::<DeletedArgs>(&body)?.deleted).await?;
                answer(serde_json::json!({}))?
            }
            "active_plans" => answer(p.active_plans(&args::<OwnersArgs>(&body)?.owners).await?)?,
            "chat_on" => answer(p.chat_on(&args::<AccountArgs>(&body)?.account).await?)?,
            "roster_changed" => {
                p.roster_changed(&args::<TeamArgs>(&body)?.team).await?;
                answer(serde_json::json!({}))?
            }
            "forge_standing" => {
                let a: StandingArgs = args(&body)?;
                answer(p.forge_standing(&a.account, a.github.as_ref(), &a.repos, a.fresh).await?)?
            }
            "github_token" => {
                let a: TokenArgs = args(&body)?;
                answer(p.github_token(&a.account, a.github.as_ref(), &a.repo).await?)?
            }
            "send_draft" => {
                let a: SendArgs = args(&body)?;
                answer(p.send_draft(&a.account, &a.draft, a.token).await?)?
            }
            "hosted_wake" => {
                let a: WakeArgs = args(&body)?;
                answer(p.hosted_wake(&a.project, &a.workspace).await?)?
            }
            "hosted_turn" => {
                let a: TurnArgs = args(&body)?;
                answer(p.hosted_turn(&a.workspace, &a.machine, &a.agent).await?)?
            }
            "hosted_turn_end" => {
                let a: TurnArgs = args(&body)?;
                answer(p.hosted_turn_end(&a.workspace, &a.machine, &a.turn).await?)?
            }
            _ => return Ok(None),
        }))
    }
    .await;
    match r {
        Ok(Some(v)) => answered(&method, Ok(v)),
        Ok(None) => not_here(&method),
        Err(e) => answered(&method, Err(e)),
    }
}

/// Only a call signed with the shared key, once, gets through.
async fn signed<P: ?Sized + Send + Sync + 'static>(
    State(s): State<Arc<Serving<P>>>,
    req: Request,
    next: Next,
) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_CALL).await else {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "an internal call is at most 4 MB").into_response();
    };
    let header = parts.headers.get(internal::HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
    let pq = parts.uri.path_and_query().map_or_else(|| parts.uri.path(), |p| p.as_str());
    let call = Call { to: s.to, method: parts.method.as_str(), path_and_query: pq, body: &bytes };
    if let Err(e) = internal::verify(&s.key, &s.nonces, header, &call, arugula_e2e::now_ms()) {
        tracing::warn!(path = pq, why = %e, "refused an internal call");
        return err(StatusCode::UNAUTHORIZED, "not a signed internal call").into_response();
    }
    next.run(Request::from_parts(parts, axum::body::Body::from(bytes))).await
}

// ---------------------------------------------------------------- the principal header

/// The platform's side of the hub's gateway: only a call the hub vouched
/// for, in the principal header signed with the shared key, gets through,
/// and what the hub vouched for becomes the request's [`Asked`], which the
/// extractors read as they do in process. Anything else is refused with
/// 401. The header goes before the handler sees the request, and the
/// hub's request id names the call in the platform's logs.
pub async fn vouched(State(key): State<PrincipalKey>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    parts.extensions.remove::<Asked>();
    let header = parts.headers.get(principal::HEADER).and_then(|v| v.to_str().ok());
    let pq = parts.uri.path_and_query().map_or_else(|| parts.uri.path().to_owned(), |p| p.as_str().to_owned());
    let checked = header
        .ok_or(principal::BadPrincipal::Malformed)
        .and_then(|h| key.verify_vouch(h, parts.method.as_str(), &pq, arugula_e2e::now_ms()));
    let (vouch, request) = match checked {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(path = parts.uri.path(), why = %e, "refused a call the hub didn't vouch for");
            return err(StatusCode::UNAUTHORIZED, "only control's hub calls the platform").into_response();
        }
    };
    parts.headers.remove(principal::HEADER);
    match vouch {
        Vouch::Principal(p) => {
            parts.extensions.insert(Asked::Principal(p));
        }
        Vouch::Not(NotVouched::Refused { status, error, by }) => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::UNAUTHORIZED);
            parts.extensions.insert(Asked::Refused { status, error, by });
        }
        Vouch::Not(NotVouched::Nobody) => {}
    }
    let span = tracing::info_span!("call", request = %request);
    next.run(Request::from_parts(parts, body)).instrument(span).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Caller, Credential, Person};

    fn key() -> PrincipalKey {
        PrincipalKey::new(&[3; 32]).unwrap()
    }

    async fn serve(router: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, router).await });
        base
    }

    /// The forwarded routes take only what the hub vouched for, as `Asked`.
    #[tokio::test]
    async fn only_what_the_hub_vouched_for_gets_through() {
        let routes = Router::new()
            .route("/person", axum::routing::get(|p: Person| async move { p.account }))
            .route("/caller", axum::routing::post(|c: Caller| async move { c.0.account().to_owned() }))
            .layer(axum::middleware::from_fn_with_state(key(), vouched));
        let base = serve(routes).await;
        let http = reqwest::Client::new();
        let now = arugula_e2e::now_ms();
        let person = Vouch::Principal(Principal::Person { account: "acct-a".into() });
        let sign = |v: &Vouch, m: &str, pq: &str| key().sign_vouch(v, m, pq, "req-1", now);
        let get = |pq: &str, h: Option<String>| {
            let mut r = http.get(format!("{base}{pq}"));
            if let Some(h) = h {
                r = r.header(principal::HEADER, h);
            }
            async move {
                let r = r.send().await.unwrap();
                (r.status().as_u16(), r.text().await.unwrap())
            }
        };

        assert_eq!(get("/person", Some(sign(&person, "GET", "/person"))).await, (200, "acct-a".into()));
        assert_eq!(get("/person?x=1", Some(sign(&person, "GET", "/person?x=1"))).await.0, 200, "with a query");
        let refused = r#"{"error":"only control's hub calls the platform"}"#.to_owned();
        assert_eq!(get("/person", None).await, (401, refused.clone()), "no header");
        assert_eq!(get("/person", Some(sign(&person, "GET", "/caller"))).await, (401, refused.clone()), "another path");
        let other = PrincipalKey::new(&[4; 32]).unwrap();
        let forged = other.sign_vouch(&person, "GET", "/person", "req-1", now);
        assert_eq!(get("/person", Some(forged)).await, (401, refused), "another key");
        // A refusal the hub vouched for is answered as the hub would have.
        let no = Vouch::Not(NotVouched::Refused {
            status: 403,
            error: "cross-origin request refused".into(),
            by: Credential::Session,
        });
        assert_eq!(
            get("/person", Some(sign(&no, "GET", "/person"))).await,
            (403, r#"{"error":"cross-origin request refused"}"#.into())
        );
        let nobody = Vouch::Not(NotVouched::Nobody);
        assert_eq!(
            get("/person", Some(sign(&nobody, "GET", "/person"))).await,
            (401, r#"{"error":"sign in first"}"#.into())
        );
        let r = http
            .post(format!("{base}/caller"))
            .header(principal::HEADER, sign(&person, "POST", "/caller"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.text().await.unwrap(), "acct-a");
    }

    /// An internal call needs the key: unsigned, signed with another key,
    /// or replayed, it's refused before it reaches the port.
    #[tokio::test]
    async fn internal_calls_need_the_key() {
        struct Plans;
        impl PlatformPort for Plans {
            fn delete_blockers<'a>(&'a self, _: &'a str, _: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
                Box::pin(async { Ok(vec!["wait".into()]) })
            }
            fn account_deleted<'a>(&'a self, _: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
                Box::pin(async { anyhow::bail!("not now") })
            }
            fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
                Box::pin(async move { Ok(owners.iter().filter(|o| o.starts_with("team:")).cloned().collect()) })
            }
            fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
                Box::pin(async move { Ok(account == "on") })
            }
            fn roster_changed<'a>(&'a self, _: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
                Box::pin(async { Ok(()) })
            }
            fn forge_standing<'a>(
                &'a self,
                _: &'a str,
                _: Option<&'a GithubUser>,
                _: &'a [String],
                _: bool,
            ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>> {
                Box::pin(async { Ok(vec![]) })
            }
            fn github_token<'a>(
                &'a self,
                _: &'a str,
                _: Option<&'a GithubUser>,
                _: &'a str,
            ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>> {
                Box::pin(async { Ok(TokenAnswer::Refused { status: 404, error: "no App".into() }) })
            }
            fn hosted_wake<'a>(
                &'a self,
                project: &'a str,
                workspace: &'a str,
            ) -> BoxFuture<'a, anyhow::Result<WakeAnswer>> {
                let why = format!("{project} {workspace}");
                Box::pin(async move {
                    Ok(WakeAnswer::Failed { code: arugula_control_wire::hosted::WakeCode::Refused, why })
                })
            }
            fn hosted_turn<'a>(
                &'a self,
                w: &'a str,
                m: &'a str,
                agent: &'a str,
            ) -> BoxFuture<'a, anyhow::Result<crate::TurnAnswer>> {
                let error = format!("{w} {m} {agent}");
                Box::pin(
                    async move { Ok(crate::TurnAnswer::Refused { status: 403, reason: Some("no_key".into()), error }) },
                )
            }
            fn hosted_turn_end<'a>(
                &'a self,
                _: &'a str,
                _: &'a str,
                t: &'a str,
            ) -> BoxFuture<'a, anyhow::Result<bool>> {
                Box::pin(async move { Ok(t == "ht_1") })
            }
        }
        let base = serve(serve_platform(Arc::new(Plans), key())).await;
        let p = HttpPlatform::new(&base, key()).unwrap();
        assert_eq!(p.active_plans(&["account:a".into(), "team:t".into()]).await.unwrap(), vec!["team:t".to_owned()]);
        assert!(p.chat_on("on").await.unwrap());
        assert!(!p.chat_on("off").await.unwrap());
        assert_eq!(p.delete_blockers("a", true).await.unwrap(), vec!["wait".to_owned()]);
        // M185: a hosted box's turn, and its end.
        assert_eq!(
            p.hosted_turn("hw_1", "m1", "claude").await.unwrap(),
            crate::TurnAnswer::Refused { status: 403, reason: Some("no_key".into()), error: "hw_1 m1 claude".into() }
        );
        assert!(p.hosted_turn_end("hw_1", "m1", "ht_1").await.unwrap());
        p.roster_changed("t").await.unwrap();
        let e = p.account_deleted(&Deleted { account: "a".into(), machines: vec![], disbanded: vec![] }).await;
        assert!(format!("{:#}", e.unwrap_err()).contains("not now"), "an error crosses as an error");
        let woke = p.hosted_wake("prj_a", "hw_a").await.unwrap();
        assert_eq!(
            woke,
            WakeAnswer::Failed { code: arugula_control_wire::hosted::WakeCode::Refused, why: "prj_a hw_a".into() }
        );

        let wrong = HttpPlatform::new(&base, PrincipalKey::new(&[9; 32]).unwrap()).unwrap();
        let e = wrong.chat_on("on").await.unwrap_err().to_string();
        assert!(e.contains("401") && e.contains("not a signed internal call"), "{e}");

        let http = reqwest::Client::new();
        let path = format!("{PLATFORM_PREFIX}/chat_on");
        let body = br#"{"account":"on"}"#;
        let r = http.post(format!("{base}{path}")).body(&body[..]).send().await.unwrap();
        assert_eq!(r.status(), 401, "unsigned");
        let sig = internal::sign(&key(), Callee::Platform, "POST", &path, body, arugula_e2e::now_ms());
        let send = || http.post(format!("{base}{path}")).header(internal::HEADER, &sig).body(&body[..]).send();
        assert_eq!(send().await.unwrap().status(), 200);
        assert_eq!(send().await.unwrap().status(), 401, "replayed");
        let hub_sig = internal::sign(&key(), Callee::Hub, "POST", &path, body, arugula_e2e::now_ms());
        let r =
            http.post(format!("{base}{path}")).header(internal::HEADER, hub_sig).body(&body[..]).send().await.unwrap();
        assert_eq!(r.status(), 401, "signed for the hub, not the platform");

        let path = format!("{PLATFORM_PREFIX}/billing_on");
        let sig = internal::sign(&key(), Callee::Platform, "POST", &path, b"{}", arugula_e2e::now_ms());
        let r = http.post(format!("{base}{path}")).header(internal::HEADER, sig).body("{}").send().await.unwrap();
        assert_eq!(r.status(), 404, "a method a newer hub might ask for");
    }
}
