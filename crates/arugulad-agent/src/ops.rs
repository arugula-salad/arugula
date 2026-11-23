//! `conversations`: the Claude Code and Codex conversations on this
//! machine, listed, continued in a terminal pane running `claude --resume`
//! or `codex resume` (Continue here) and opened as agent blocks (M33, `arugula claude`), the owner's (`serve`
//! checks that before the handler runs). MCP's `list` kind
//! conversations and `show` kind conversation are the same two operations: an
//! agent's token is confined to its machine and its tab (`McpOp::args`), and
//! opens one as no one in particular. They're the agent block's
//! (`Handle<AgentBlock>`, routed with `op_of`), and MCP lists them through
//! `mcp::registry` where they were.

use std::collections::HashMap;

use arugula_proto::{
    PaneId,
    api::{
        ContinueConversationRequest, ContinueConversationResponse, ConversationList, ConversationRow,
        ConversationSignal, ConversationsQuery, OpenConversationRequest, OpenConversationResponse, SessionSignal,
        SessionSignaled,
    },
    op::ops::{
        ConversationContinue, ConversationOpen, ConversationSessionSignal, ConversationsList, PaneSessionSignal,
    },
};
use axum::http::StatusCode;
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use arugulad_base::ops::{Cx, Handle, OpError, Via};

use super::AgentBlock;
use crate::{
    api::{ApiError, bad},
    mcp::{
        ops::{McpOp, Surface, kind_of},
        tools::{Call, Kind, Out, PaneArg, confine, done},
    },
    mux::Api,
    server::App,
};

#[derive(Deserialize, JsonSchema)]
pub struct ListConversationsArgs {
    /// Words in the title, prompts or folder.
    #[serde(default)]
    pub query: Option<String>,
    /// Only ones under this folder.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Only ones open somewhere now.
    #[serde(default)]
    pub live: bool,
    /// Everything: `claude -p` and SDK runs, archived ones, ones whose folder is gone.
    #[serde(default)]
    pub all: bool,
    /// How many [default 30].
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenConversationArgs {
    /// The conversation's id, or the start of it (list kind conversations).
    pub id: String,
    /// Then continue it, or fork it (for one open somewhere else) and go on in the fork.
    #[serde(default)]
    pub then: Option<ConversationThen>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ConversationThen {
    Continue,
    Fork,
}

/// `list` kind conversations, after panes, and `show` kind conversation,
/// after issue: where they were.
pub(super) fn register() {
    use crate::mcp::{ops::call_of, registry};
    static LIST: Kind = kind_of::<ConversationsList, AgentBlock>();
    static SHOW: Kind = kind_of::<ConversationOpen, AgentBlock>();
    fn list<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
        Box::pin(call_of::<ConversationsList, AgentBlock>(c, args))
    }
    fn show<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
        Box::pin(call_of::<ConversationOpen, AgentBlock>(c, args))
    }
    registry::kind(registry::ToolKind { tool: "list", after: Some("panes"), kind: &LIST, run: list });
    registry::kind(registry::ToolKind { tool: "show", after: Some("issue"), kind: &SHOW, run: show });
}

impl Handle<AgentBlock> for ConversationsList {
    async fn handle(cx: &Cx<'_>, _: (), q: ConversationsQuery) -> Result<ConversationList, OpError> {
        Ok(list_conversations(cx.app, q).await.map_err(bad)?)
    }
}

impl Handle<AgentBlock> for ConversationOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: OpenConversationRequest) -> Result<OpenConversationResponse, OpError> {
        let who = match &cx.via {
            Via::Http { who, .. } => Some(who.clone()),
            Via::Mcp(_) => None,
        };
        Ok(open_conversation_as(cx.app, who, req).await?)
    }
}

impl Handle<AgentBlock> for ConversationContinue {
    async fn handle(
        cx: &Cx<'_>,
        _: (),
        req: ContinueConversationRequest,
    ) -> Result<ContinueConversationResponse, OpError> {
        Ok(continue_conversation(cx.app, req).await?)
    }
}

impl Handle<AgentBlock> for PaneSessionSignal {
    /// From an older `arugula` (no pid), or a page: its pane, as before.
    async fn handle(cx: &Cx<'_>, id: PaneId, s: SessionSignal) -> Result<SessionSignaled, OpError> {
        Ok(session_signal(cx.app, s, Some(id), None).await?)
    }
}

impl Handle<AgentBlock> for ConversationSessionSignal {
    async fn handle(cx: &Cx<'_>, _: (), s: ConversationSignal) -> Result<SessionSignaled, OpError> {
        if s.signal.session_id.as_deref().is_none_or(str::is_empty) {
            return Err(bad("a session signal for a conversation needs Claude Code's session id").into());
        }
        Ok(session_signal(cx.app, s.signal, s.pane, s.pid).await?)
    }
}

/// What Claude Code said of its session from inside it (#396, #453): the
/// mod's signals, read into the same context an agent block reads from its
/// transcript (#397), kept by session ([`signals`]). Logged, listed with
/// its conversation, and given to the mux as the `live` of the pane it
/// runs in (#450), which the strip shows.
///
/// [`signals`]: crate::conversations::signals
async fn session_signal(
    app: &App,
    s: SessionSignal,
    pane: Option<PaneId>,
    pid: Option<u32>,
) -> Result<SessionSignaled, ApiError> {
    use crate::conversations::signals::{Hint, Signals, place};
    let Some(key) = Signals::key(&s, pane) else {
        return Err(bad("no session id and no pane: nothing to keep it by"));
    };
    // The Claude Code that sent it: the CLI's parent.
    let claude = pid.and_then(crate::procinfo::ppid);
    let hint = match pane {
        None => Hint::None,
        Some(p) => match app.mux.api(|r| Api::Pane(p, r)).await.flatten() {
            Some(h) => {
                let leader = h.pid_now();
                // In the pane's foreground job, or in the pane a `tmux`
                // client there shows (which asks `tmux`).
                let front = match pid {
                    None => None,
                    Some(pid) => Some(match leader {
                        Some(l) => crate::task::spawn_blocking(move || {
                            crate::procinfo::in_front(l, pid, crate::conversations::signals::tmux_pane)
                        })
                        .await
                        .unwrap_or(false),
                        None => false,
                    }),
                };
                Hint::Terminal(p, front)
            }
            None => match app.mux.api(|r| Api::Block(p, r)).await.flatten() {
                Some(b) => Hint::Block(p, b.config()["session_id"].as_str().map(str::to_owned)),
                None => Hint::None,
            },
        },
    };
    let session = s.session_id.as_deref().filter(|id| !id.is_empty());
    let held = match (session, &hint) {
        (Some(id), Hint::None | Hint::Block(..)) => blocks_by_session(app).await.get(id).copied(),
        _ => None,
    };
    let at = place(session, &hint, held);
    let context = Signals::global().lock().unwrap_or_else(|p| p.into_inner()).take(&key, &s, crate::store::now_ms());
    tracing::info!(
        target: "arugulad_agent::session_signal",
        session = %key,
        pane = ?pane,
        shows_on = ?at.pane(),
        event = %s.event,
        signal = %serde_json::to_string(&s).unwrap_or_default(),
        context = %context,
        "session signal"
    );
    if let Some(p) = at.pane() {
        let live = (!context.is_null()).then(|| context.clone());
        // A terminal keeps it while that Claude Code is in front (#453).
        let by = matches!(at, crate::conversations::signals::Place::Pane(_)).then_some(claude).flatten();
        app.mux.send(crate::mux::Cmd::Api(Api::Live(p, live, by)));
    }
    Ok(SessionSignaled { daemon_version: arugula_core::version::get().to_owned(), context, pane: at.pane() })
}

impl McpOp<AgentBlock> for ConversationsList {
    const SURFACE: Surface = Surface::Kind {
        tool: "list",
        kind: "conversations",
        description: "Claude Code conversations on this machine, from a terminal or the desktop app's Code tab, and Codex's (agent codex), newest first: id, title, folder, first and last prompt, where it's open now, and the agent block that has it. show kind conversation opens one.",
    };
    type Args = ListConversationsArgs;
    type Answer = serde_json::Value;

    async fn args(_: &Call<'_>, a: &ListConversationsArgs) -> Result<((), ConversationsQuery), String> {
        let q = ConversationsQuery {
            all: a.all,
            q: a.query.clone(),
            cwd: a.cwd.clone(),
            live: a.live,
            limit: Some(a.limit.unwrap_or(30)),
        };
        Ok(((), q))
    }

    fn answer(_: &Call<'_>, _: &ListConversationsArgs, _: &(), res: ConversationList) -> Out {
        let mut v = serde_json::to_value(res).unwrap_or_default();
        // The transcript's path is the daemon's business.
        for c in v["conversations"].as_array_mut().into_iter().flatten() {
            if let Some(o) = c.as_object_mut() {
                o.remove("path");
            }
        }
        let n = v["conversations"].as_array().map_or(0, Vec::len);
        done(format!("{n} conversations; show (kind conversation) shows one as a block"), v)
    }
}

impl McpOp<AgentBlock> for ConversationOpen {
    const SURFACE: Surface = Surface::Kind {
        tool: "show",
        kind: "conversation",
        description: "A Claude Code conversation (list kind conversations) as an agent block, stopped, with its transcript; then: continue (refused while it's open somewhere else) or fork (a new session with its history; the original is left alone). send_input to the block goes on. A Codex one goes on in a terminal pane running codex resume.",
    };
    type Args = OpenConversationArgs;
    type Answer = serde_json::Value;

    async fn args(call: &Call<'_>, a: &OpenConversationArgs) -> Result<((), OpenConversationRequest), String> {
        // Conversations are this host's, and continue as a Claude Code here.
        confine(call.on_machine().await?, false)?;
        let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, call.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        if let Some(b) = beside {
            call.readable(b).await?;
        }
        let req = OpenConversationRequest {
            id: a.id.clone(),
            then: a.then.map(|t| match t {
                ConversationThen::Continue => "continue".into(),
                ConversationThen::Fork => "fork".into(),
            }),
            session: None,
            split: beside,
            from_pane: beside,
        };
        Ok(((), req))
    }

    fn answer(_: &Call<'_>, _: &OpenConversationArgs, _: &(), v: OpenConversationResponse) -> Out {
        let block = v.block;
        let summary = match v.error.as_deref() {
            Some(e) => format!("Opened it in %{block}, but: {e}"),
            None => format!("It's in %{block}; send_input to it to go on, then wait and read_output"),
        };
        done(summary, serde_json::to_value(&v).unwrap_or_default())
    }
}

/// Agent blocks and chats by the Claude Code session they have.
async fn blocks_by_session(app: &App) -> HashMap<String, PaneId> {
    let mut out = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if !p.info.kind.prompted() {
            continue;
        }
        if let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(sid) = b.config()["session_id"].as_str()
        {
            out.insert(sid.to_owned(), p.info.id);
        }
    }
    out
}

/// Our terminals' and agent blocks' processes on this host (#81): a
/// Claude Code under one of them runs there.
async fn our_pids(app: &App) -> crate::conversations::Ours {
    let mut ours = crate::conversations::Ours::default();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        let id = p.info.id;
        match p.info.kind {
            arugula_proto::BlockType::Terminal => {
                if let Some(pid) = app.mux.api(|r| Api::Pane(id, r)).await.flatten().and_then(|h| h.pid_now()) {
                    ours.panes.insert(pid, id);
                }
            }
            _ => {
                if let Some(pid) = app.mux.api(|r| Api::Block(id, r)).await.flatten().and_then(|b| b.pid()) {
                    ours.blocks.insert(pid, id);
                }
            }
        }
    }
    ours
}

async fn list_conversations(app: &App, q: ConversationsQuery) -> Result<ConversationList, String> {
    let blocks = blocks_by_session(app).await;
    let ours: std::collections::HashSet<PaneId> =
        app.mux.api(Api::Panes).await.unwrap_or_default().into_iter().map(|p| p.info.id).collect();
    let pids = our_pids(app).await;
    let list = crate::task::spawn_blocking(move || {
        let mut ix = crate::conversations::Index::global().lock().unwrap();
        ix.set_ours(pids);
        ix.scan()
    })
    .await
    .map_err(|e| e.to_string())?;
    let signaled = crate::conversations::signals::Signals::global().lock().unwrap_or_else(|p| p.into_inner());
    let words: Vec<String> = q.q.as_deref().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let total = list.len();
    let out: Vec<ConversationRow> = list
        .into_iter()
        .filter(|c| crate::conversations::shown(c, q.all))
        .filter(|c| !q.live || c.live.is_some())
        .filter(|c| q.cwd.as_deref().is_none_or(|d| crate::paths::is_under(&c.cwd, d)))
        .filter(|c| {
            let hay = format!(
                "{} {} {} {}",
                c.title,
                c.first_prompt.as_deref().unwrap_or(""),
                c.last_prompt.as_deref().unwrap_or(""),
                c.cwd
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(q.limit.unwrap_or(500))
        .map(|mut c| {
            // #398: its estimate, once the pricing thread has read it.
            // Claude's prices, so Claude Code's transcripts only.
            if c.agent == "claude" {
                c.estimate = crate::conversations::spend::Listed::global().get(&c.path, (c.size, c.updated_ms));
            }
            // #453: what Claude Code said of it from inside, live.
            c.context = signaled.get(&c.id);
            let block = blocks.get(&c.id).copied();
            c.live = c.live.take().map(|l| l.ours(|p| ours.contains(&p)));
            // The adapter of the block that has it, when its scope didn't
            // say (no systemd scopes).
            if let (Some(b), Some(l)) = (block, c.live.as_mut())
                && l.block.is_none()
                && l.entrypoint == "sdk-ts"
            {
                l.block = Some(b);
                l.pane = None;
                l.place = l.place();
            }
            ConversationRow { conversation: serde_json::to_value(&c).unwrap_or_default(), block }
        })
        .collect();
    Ok(ConversationList { conversations: out, total })
}

async fn open_conversation_as(
    app: &App,
    who: Option<crate::acl::Principal>,
    req: OpenConversationRequest,
) -> Result<OpenConversationResponse, ApiError> {
    let id = req.id.clone();
    let c = crate::task::spawn_blocking(move || crate::conversations::Index::global().lock().unwrap().find(&id))
        .await
        .map_err(|e| bad(e.to_string()))?
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, e))?;
    if c.agent == "codex" {
        return continue_codex(app, c, req).await;
    }
    let existing = blocks_by_session(app).await.get(&c.id).copied();
    let (block, opened) = match existing {
        Some(b) => (b, false),
        None => {
            let source = serde_json::to_value(c.source).unwrap_or_default();
            let config = serde_json::json!({
                "agent": "claude",
                "cwd": c.cwd,
                "session_id": c.id,
                "import": { "path": c.path, "source": source, "title": c.title, "model": c.model },
            });
            let open = arugula_proto::api::OpenRequest {
                kind: arugula_proto::BlockType::Agent,
                config,
                session: req.session,
                split: req.split,
                from_pane: req.from_pane,
                vm: false,
                image: None,
                host: None,
                local: true,
                project: None,
                issue: None,
            };
            match app.mux.api(|r| Api::Open(open, who, r)).await {
                Some(Ok(b)) => (b, true),
                Some(Err(e)) => return Err(bad(e)),
                None => return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
            }
        }
    };
    let mut out = OpenConversationResponse { block, opened, conversation: c.id, error: None };
    if let Some(then) = req.then.as_deref() {
        let method = match then {
            "continue" | "fork" => then,
            t => return Err(bad(format!("then: {t}? (continue or fork)"))),
        };
        let b = app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| bad("the block went away"))?;
        if let Err(e) = b.call(method, serde_json::json!({})).await {
            out.error = Some(e);
        }
    }
    Ok(out)
}

/// A Codex conversation opened (MCP's show kind conversation, an older
/// client's Open): there's no transcript view of one yet, so it's
/// Continue here's Codex case, a terminal pane running `codex resume`.
/// Forking is refused.
async fn continue_codex(
    app: &App,
    c: crate::conversations::Conversation,
    req: OpenConversationRequest,
) -> Result<OpenConversationResponse, ApiError> {
    if req.then.as_deref() == Some("fork") {
        return Err(bad("a Codex conversation can't be forked here yet: continue it"));
    }
    let v = continue_conversation(
        app,
        ContinueConversationRequest { id: c.id, home: false, session: req.session, from_pane: req.from_pane },
    )
    .await?;
    match v.pane {
        Some(pane) => Ok(OpenConversationResponse { block: pane, opened: v.opened, conversation: v.conversation, error: None }),
        None => Err(bad(format!("its folder, {}, is gone", if v.cwd.is_empty() { "unrecorded" } else { &v.cwd }))),
    }
}

/// Where and how Continue here resumes `agent`'s conversation `id`: the
/// folder, and the command its pane's shell runs, the person's own
/// `claude` or `codex` on their `PATH` ([`arugula_mux::resume::command`]).
/// The id is checked before it goes into a command line, so nothing in it
/// reaches the shell as anything but itself. `None` for the folder when
/// it's gone and home wasn't asked for.
fn resume_here(
    agent: &str,
    id: &str,
    cwd: &str,
    cwd_exists: bool,
    home: Option<&std::path::Path>,
) -> Result<(Option<String>, String), String> {
    let argv = arugula_mux::resume::command(agent, id)
        .filter(|_| arugula_mux::resume::valid_id(id))
        .ok_or_else(|| format!("{id:?} isn't a {} session id", arugula_mux::resume::name(agent)))?;
    let dir = match (cwd_exists && !cwd.is_empty(), home) {
        (true, _) => Some(cwd.to_owned()),
        (false, Some(h)) => Some(h.display().to_string()),
        (false, None) => None,
    };
    Ok((dir, argv.join(" ")))
}

/// Continue here, Claude Code's or Codex's: the conversation's own pane or
/// block if one of ours has it, else a new tab in its folder running its
/// agent's resume command ([`resume_in_pane`]). One that's open outside
/// Arugula is refused: two agents on one conversation write over each
/// other.
async fn continue_conversation(
    app: &App,
    req: ContinueConversationRequest,
) -> Result<ContinueConversationResponse, ApiError> {
    let id = req.id.clone();
    let pids = our_pids(app).await;
    let ours: std::collections::HashSet<PaneId> =
        app.mux.api(Api::Panes).await.unwrap_or_default().into_iter().map(|p| p.info.id).collect();
    let c = crate::task::spawn_blocking(move || {
        let mut ix = crate::conversations::Index::global().lock().unwrap();
        ix.set_ours(pids);
        ix.find(&id)
    })
    .await
    .map_err(|e| bad(e.to_string()))?
    .map_err(|e| ApiError(StatusCode::NOT_FOUND, e))?;
    let live = c.live.clone().map(|l| l.ours(|p| ours.contains(&p)));
    let there = blocks_by_session(app).await.get(&c.id).copied().or(live.as_ref().and_then(|l| l.pane.or(l.block)));
    let answer = |pane, opened, cwd: String, folder_gone| ContinueConversationResponse {
        pane,
        opened,
        conversation: c.id.clone(),
        cwd,
        folder_gone,
    };
    if let Some(p) = there {
        return Ok(answer(Some(p), false, c.cwd.clone(), false));
    }
    if let Some(l) = &live {
        let or = if c.agent == "claude" { ", or open it as an agent block and fork it" } else { "" };
        return Err(ApiError(StatusCode::CONFLICT, format!("it's {}: go on there{or}", l.place)));
    }
    let home = if req.home {
        Some(app.mux.api(Api::AgentEnv).await.map(|(h, _)| h).unwrap_or_else(crate::home))
    } else {
        None
    };
    let (dir, command) = resume_here(c.agent, &c.id, &c.cwd, c.cwd_exists, home.as_deref()).map_err(bad)?;
    let Some(dir) = dir else {
        return Ok(answer(None, false, c.cwd.clone(), true));
    };
    let pane = resume_in_pane(app, &c.title, command, dir.clone(), req.session, req.from_pane).await?;
    Ok(answer(Some(pane), true, dir, false))
}

/// A new tab in `dir` whose pane's shell runs `command` (a resume, checked
/// by [`resume_here`]), named after the conversation's `title`.
async fn resume_in_pane(
    app: &App,
    title: &str,
    command: String,
    dir: String,
    session: Option<String>,
    from_pane: Option<PaneId>,
) -> Result<PaneId, ApiError> {
    let run = arugula_proto::api::RunRequest {
        command: Some(command),
        session,
        cwd: Some(dir),
        from_pane,
        ..Default::default()
    };
    let pane = match app.mux.api(|r| Api::Run(run, r)).await {
        Some(Ok(p)) => p,
        Some(Err(e)) => return Err(bad(e)),
        None => return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    };
    // A tab of its own is what `run` made; this names it.
    let title = Some(title.to_owned()).filter(|t| !t.trim().is_empty());
    if let Some(Err(e)) = app.mux.api(|r| Api::OwnTab(pane, title, r)).await {
        tracing::warn!(pane, "naming the tab of a continued conversation: {e}");
    }
    Ok(pane)
}

#[cfg(test)]
mod tests {
    use super::resume_here;

    /// Continue here runs the person's own `claude --resume <id>` in the
    /// conversation's folder; with the folder gone, nowhere until home is
    /// asked for, then there; and an id that isn't one never reaches the
    /// shell.
    #[test]
    fn continue_here_resumes_in_its_folder() {
        let id = "0b8f6c1e-2a4d-4e2b-9d1a-7c3e5f6a8b90";
        let home = std::path::Path::new("/home/sam");
        let want = format!("claude --resume {id}");
        let here = |cwd, exists, home| resume_here("claude", id, cwd, exists, home);
        assert_eq!(here("/home/sam/dev/x", true, None), Ok((Some("/home/sam/dev/x".into()), want.clone())));
        assert_eq!(here("/home/sam/dev/x", true, Some(home)), Ok((Some("/home/sam/dev/x".into()), want.clone())));
        assert_eq!(here("/home/sam/gone", false, None), Ok((None, want.clone())));
        assert_eq!(here("/home/sam/gone", false, Some(home)), Ok((Some("/home/sam".into()), want)));
        for bad in ["x; rm -rf ~", "--help", "$(id)", ""] {
            assert!(resume_here("claude", bad, "/tmp", true, None).is_err(), "{bad}");
        }
        assert!(resume_here("aider", id, "/tmp", true, None).is_err());
    }

    /// A Codex one the same way, with `codex resume <id>`.
    #[test]
    fn continuing_codex_resumes_it_in_its_folder() {
        let id = "01a206ce-0c41-7ce0-a62f-517a7a089b86";
        let want = format!("codex resume {id}");
        assert_eq!(resume_here("codex", id, "/home/sam/dev/x", true, None), Ok((Some("/home/sam/dev/x".into()), want.clone())));
        assert_eq!(resume_here("codex", id, "/home/sam/gone", false, None), Ok((None, want.clone())));
        assert_eq!(
            resume_here("codex", id, "/home/sam/gone", false, Some(std::path::Path::new("/home/sam"))),
            Ok((Some("/home/sam".into()), want))
        );
        assert!(resume_here("codex", "x; rm -rf ~", "/tmp", true, None).is_err());
    }
}
