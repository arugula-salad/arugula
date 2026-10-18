//! The tools (M16): about twenty, shaped for agents rather than mirroring
//! every endpoint; the ones that do several jobs (list, show, draft) take a
//! `kind`, and history's `kind` takes `output` for what panes printed, so a
//! client's list stays short (#349). Each answers with a JSON object that has a `summary`
//! sentence in it (the text block is the same JSON), or fails with a
//! sentence an agent can act on (`isError`), never a protocol error.

use std::{future::Future, sync::Arc, time::Duration};

use arugula_proto::{
    Attention, BlockType, Driver, PaneId, Policy, SessionId, StartedBy, ThreadTarget,
    api::{ActRequest, HistoryEntry, HistoryKind, OpenRequest, PaneSummary, RunRequest, WaitResult},
    flags::{self, On},
    forge::{ForgeState, ItemKind, ReviewEvent, Write},
    op::ops::{ClosePane, ConversationOpen, ConversationsList, ListPanes, ThreadGet, ThreadPost},
};
use rmcp::{
    Peer, RoleServer,
    handler::server::common::schema_for_type,
    model::{CallToolResult, ContentBlock, ProgressNotificationParam, ProgressToken, Tool, ToolAnnotations},
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    Caller, Scope,
    results::{self, RunState},
};
use crate::{
    history::{self, Filter},
    mux::Api,
    ops::{Cx, Handle, Via},
    osc::strip,
    pane::{CaptureFormat, CaptureScope},
    server::App,
    store::{PaneLog, now_ms},
};

/// A page of output, by default (characters).
const PAGE: usize = 16_000;
/// The most any one result carries: Claude Code swaps anything over about
/// 50,000 for a preview and a file (S14).
const PAGE_MAX: usize = 40_000;
/// How long a wait (or `run` with `wait`) waits before answering "still
/// running": under interactive Claude Code's 120s auto-background.
const WAIT_DEFAULT: Duration = Duration::from_secs(100);
const WAIT_MAX: Duration = Duration::from_secs(3600);
/// Progress while waiting: over HTTP, Claude Code drops a call that's
/// silent for 60s.
const PROGRESS_EVERY: Duration = Duration::from_secs(15);
/// The last lines a finished command's result carries.
const TAIL_LINES: usize = 40;
/// Invites one caller may have waiting for the user (#234).
const INVITES_WAITING: u64 = 5;
/// Permission modes that approve everything (Claude Code's, codex-acp's):
/// start_agent won't start an agent in one (#163).
const SKIPS_CHECKS: &[&str] = &["bypassPermissions", "full-access"];

fn progress_every() -> Duration {
    // Tests make it short.
    std::env::var("ARUGULA_MCP_PROGRESS_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(PROGRESS_EVERY)
}

// ---------------------------------------------------------------- arguments

/// What `open_forge` asks the forge block's `open_config` for (a `Value` it
/// keeps, being the block's persisted config): a PR or issue by reference,
/// or a new issue. Only what is set goes in.
#[derive(Debug, Default, Serialize)]
struct ForgeOpen {
    #[serde(skip_serializing_if = "Option::is_none")]
    pr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dir: Option<String>,
    /// The agent's own pane (a new issue's draft), for *Ask for changes*.
    #[serde(skip_serializing_if = "Option::is_none")]
    pane: Option<PaneId>,
}

/// A pane or block: `7` or `"%7"`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PaneArg {
    Id(PaneId),
    Name(String),
}

impl PaneArg {
    pub(crate) fn id(&self) -> Result<PaneId, String> {
        match self {
            PaneArg::Id(n) => Ok(*n),
            PaneArg::Name(s) => s
                .trim()
                .trim_start_matches('%')
                .parse()
                .map_err(|_| format!("{s:?} isn't a pane: give its number, like 7 or \"%7\"")),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct RunArgs {
    /// The command line, typed into a new shell (so it's in history, and the
    /// shell stays for you to take over). None: just a shell.
    #[serde(default)]
    pub command: Option<String>,
    /// Where it starts: a directory, on the machine it runs on.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Split this pane instead of opening a new tab; "self" is the caller's
    /// own pane. An agent block's token always splits (beside the agent, by
    /// default).
    #[serde(default)]
    pub split: Option<PaneArg>,
    /// With split: run where that pane runs (its machine) instead of on this
    /// host.
    #[serde(default)]
    pub join: bool,
    /// On a new throwaway VM of its own, deleted when the pane closes.
    #[serde(default)]
    pub vm: bool,
    /// In a new tab whose panes share a new throwaway VM.
    #[serde(default)]
    pub vm_tab: bool,
    /// The VM's image (the provider's default if none).
    #[serde(default)]
    pub image: Option<String>,
    /// On an existing sandbox, by the provider's name for it.
    #[serde(default)]
    pub machine: Option<String>,
    /// The session (by name or id) for a new tab; made if it doesn't exist.
    #[serde(default)]
    pub session: Option<String>,
    /// After a reboot: `shell` (default), `none`, `rerun` or `rerun-ask`.
    #[serde(default)]
    pub policy: Option<String>,
    /// Wait for the command to finish (up to timeout); else return at once.
    #[serde(default)]
    pub wait: bool,
    /// Seconds to wait before answering "still running" (default 100).
    #[serde(default)]
    pub timeout: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SendArgs {
    pub pane: PaneArg,
    /// Text to type (to an agent block: its next prompt).
    #[serde(default)]
    pub text: Option<String>,
    /// Press Enter after the text (default true).
    #[serde(default)]
    pub enter: Option<bool>,
    /// Named keys, after the text: C-c, C-d, Up, Down, Enter, Escape, Tab,
    /// F5, M-x, or single characters.
    #[serde(default)]
    pub keys: Vec<String>,
    /// To an app block: which of its box's chat tabs (title or chat key);
    /// default the first.
    #[serde(default)]
    pub tab: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AttachArgs {
    pub pane: PaneArg,
    /// A file on this host (a screenshot, a log). An agent on a machine of
    /// its own sends data instead.
    #[serde(default)]
    pub path: Option<String>,
    /// Or the file itself, base64.
    #[serde(default)]
    pub data: Option<String>,
    /// With data: its extension (png, jpg, txt); an image's is found.
    #[serde(default)]
    pub ext: Option<String>,
    /// To an agent block: a prompt to send with it.
    #[serde(default)]
    pub text: Option<String>,
    /// Into a terminal: paste the path even if what's in front isn't a
    /// shell or an agent.
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    pub pane: PaneArg,
    /// Where to start (a stream offset: a previous result's next_offset).
    /// Without it: the latest output, or with last_command, its output.
    #[serde(default)]
    pub offset: Option<u64>,
    /// The output of the pane's last (or current) command.
    #[serde(default)]
    pub last_command: bool,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
    /// What the pane shows right now instead, as text (for full-screen
    /// programs); the other arguments don't apply.
    #[serde(default)]
    pub screen: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct PaneOnly {
    pub pane: PaneArg,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    /// The command running (or the next one to start) finished.
    CommandEnd,
    /// The pane's process exited.
    Exit,
    /// The output matched `pattern` (a regex).
    Match,
    /// It's not working any more (an agent finished its turn, say).
    Idle,
    /// It asks for you: an agent's approval or question.
    NeedsInput,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitArgs {
    pub pane: PaneArg,
    pub until: Until,
    /// For until: match, a regular expression.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Seconds before answering "still running" (default 100).
    #[serde(default)]
    pub timeout: Option<f64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ListArgs {}

#[derive(Deserialize, JsonSchema)]
pub struct ThreadArgs {
    /// The pane whose thread it is (`7` or `"%7"`). Neither this nor a session: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// Or a session's thread, by its id.
    #[serde(default)]
    pub session: Option<SessionId>,
    /// Only messages after this id (what an earlier call returned as last).
    #[serde(default)]
    pub after: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct PostThreadArgs {
    /// The pane whose thread to post in (`7` or `"%7"`). Neither this nor a session: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// Or a session's thread, by its id.
    #[serde(default)]
    pub session: Option<SessionId>,
    /// The message. `@name` mentions someone (they're notified).
    pub text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct HistoryArgs {
    /// Only commands that failed (exit code not 0); answers and agent
    /// steps never count.
    #[serde(default)]
    pub failed: bool,
    /// `command` (ran in a shell), `answer` (an answer or approval, with
    /// who gave it) or `agent` (an agent block's non-shell steps). Without
    /// it, all three. Or `output`: lines of output across panes (and agents'
    /// transcripts) matching pattern, with the pane and command each came
    /// from; it takes pattern, since and limit.
    #[serde(default)]
    pub kind: Option<String>,
    /// For kind output: a regular expression, matched against each line of
    /// output.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Started at most this long ago: `90m`, `36h`, `2d`, or seconds.
    #[serde(default)]
    pub since: Option<String>,
    /// Started at least this long ago (with since: a window, like since
    /// 2d before 1d for yesterday).
    #[serde(default)]
    pub before: Option<String>,
    /// Ran in this directory or below it.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The command line matches this regex.
    #[serde(rename = "match", default)]
    pub matching: Option<String>,
    /// One pane's.
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// At most this many, newest first (default 30, at most 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// A regular expression, matched against each line of output.
    pub pattern: String,
    /// Output from at most this long ago: `90m`, `36h`, `2d`, or seconds.
    #[serde(default)]
    pub since: Option<String>,
    /// At most this many lines (default 30, at most 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenPortArgs {
    /// The port, on the machine the pane runs on.
    pub port: u16,
    /// The page's path (default /).
    #[serde(default)]
    pub path: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

// Read only by the handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct OpenAppArgs {
    /// The app's name in the user's studio. Leave it out to list their
    /// apps instead.
    #[serde(default)]
    pub app: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Claude Code.
    #[default]
    Claude,
    Codex,
    /// An agent in a Managoat sandbox (managoat_agent names it).
    // `fountain`, its name before the rename, is still read (not listed).
    #[serde(rename = "managoat", alias = "fountain")]
    Fountain,
    /// Any ACP agent server, by command.
    Acp,
}

#[derive(Deserialize, JsonSchema)]
pub struct StartAgentArgs {
    #[serde(default)]
    pub agent: AgentKind,
    /// What to ask it.
    pub prompt: String,
    /// For acp: the agent server's command line.
    #[serde(default)]
    pub command: Option<String>,
    /// For managoat: the agent's name or id.
    #[serde(default, rename = "managoat_agent", alias = "fountain_agent")]
    pub fountain_agent: Option<String>,
    /// For claude: run as this recipe, a Claude Code subagent by its name
    /// (`.claude/agents/NAME.md` in cwd's project, or the user's), on this
    /// host: its prompt, model, tools, skills and MCP servers.
    #[serde(default)]
    pub recipe: Option<String>,
    /// A model to switch to (`haiku`, ...).
    #[serde(default)]
    pub model: Option<String>,
    /// A short name for the block, shown instead of the agent's name.
    #[serde(default)]
    pub title: Option<String>,
    /// Where it works.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Tools whose requests it may make without asking (`Read`, `Edit`,
    /// `Bash`, ...), as "always" on a card.
    #[serde(default)]
    pub allow: Vec<String>,
    /// The permission mode its session starts in: `default`, `acceptEdits`,
    /// `plan`, `auto` (Claude Code's), or the agent's own. Not one that
    /// skips every check: the user picks that.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// For claude: the user's Claude Code settings (allow and deny lists,
    /// default mode, CLAUDE.md), without their hooks.
    #[serde(default)]
    pub user_settings: bool,
    /// Open it beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
    /// The session for a new tab, when not beside a pane.
    #[serde(default)]
    pub session: Option<String>,
    /// On a new throwaway VM of its own.
    #[serde(default)]
    pub vm: bool,
    /// Where a recipe runs: here (default), or managoat: in a Managoat sandbox, as its Managoat agent recipe-NAME (upserted from the recipe first), with its project's origin cloned there (uncommitted work stays here).
    #[serde(default)]
    pub on: Option<String>,
    /// For on: managoat: a Managoat vault for the run (name or id).
    #[serde(default)]
    pub vault: Option<String>,
    /// For on: managoat: a Managoat environment instead of the project's (name or id).
    #[serde(default)]
    pub environment: Option<String>,
}

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

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    /// Approve the pending permission request.
    Allow,
    /// Refuse it (message: why).
    Deny,
    /// Answer the pending question (answers: its fields).
    Answer,
    /// Skip the pending question.
    Skip,
}

#[derive(Deserialize, JsonSchema)]
pub struct PromptArgs {
    /// An agent block, or a terminal running an agent (Claude Code, Codex).
    pub pane: PaneArg,
    /// The prompt.
    pub text: String,
    /// It's waiting on an approval or a question and this answers it.
    /// Without it, an agent waiting on someone isn't typed at: its
    /// question comes back instead.
    #[serde(default)]
    pub answering: bool,
    /// Seconds before answering "still running" (default 100): then wait
    /// until idle.
    #[serde(default)]
    pub timeout: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RespondArgs {
    /// The agent block, or a terminal running Claude Code.
    pub pane: PaneArg,
    pub action: Response,
    /// allow: `once` (default) or `always`.
    #[serde(default)]
    pub option: Option<String>,
    /// deny: why, for the agent.
    #[serde(default)]
    pub message: Option<String>,
    /// answer: the question's fields, by name, as wait (until needs_input)
    /// showed them, e.g. {"question_0": "Blue"}.
    #[serde(default)]
    pub answers: Option<serde_json::Map<String, Value>>,
    /// Which request it answers (from wait); default whatever it asks now.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    /// The file. Relative paths are from the pane's directory, or home.
    pub path: String,
    /// On the machine this pane runs on. Default: the caller's own pane (an
    /// agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// A byte offset to start at (a previous result's next_offset).
    #[serde(default)]
    pub offset: Option<u64>,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ShowChangesArgs {
    /// The pane whose repository it is (its directory, on its machine). Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
    /// Any directory in the repository, instead (on the pane's machine).
    #[serde(default)]
    pub repo: Option<String>,
    /// Compare the working tree with this revision instead of HEAD.
    #[serde(default)]
    pub rev_a: Option<String>,
    /// With rev_a: the range rev_a..rev_b instead of the working tree.
    #[serde(default)]
    pub rev_b: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ShowFileArgs {
    /// The file. Relative paths are from the pane's directory, or home.
    pub path: String,
    /// The line to mark and scroll to.
    #[serde(default)]
    pub line: Option<u32>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

/// One section of a Doc's outline.
#[derive(Deserialize, JsonSchema)]
pub struct DocSection {
    /// The section's heading.
    pub heading: String,
    /// One line on what the section will hold: the user reads it while the section is pending.
    #[serde(default)]
    pub intent: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocNewArgs {
    /// The Doc's title.
    pub title: String,
    /// Every section, in order (at most 40), so the user sees the whole outline at once.
    pub sections: Vec<DocSection>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocFillArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
    /// The section's id (s1, s2, … from doc new).
    pub section: String,
    /// The section's body, in Markdown (at most 64 KB). Filling a section again replaces it.
    pub markdown: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocAssetArgs {
    /// The Doc block (from doc new).
    pub block: PaneArg,
    /// The picture's name in the Doc: letters, digits, _ . and -, ending in .png, .jpg, .jpeg, .gif or .webp (the bytes must be that type). A name already used is replaced.
    pub name: String,
    /// A PNG, JPEG, GIF or WebP file you can read, as an absolute path (at most 2 MB). An agent on a machine of its own sends data instead.
    #[serde(default)]
    pub path: Option<String>,
    /// Or the file itself, base64 (at most 2 MB).
    #[serde(default)]
    pub data: Option<String>,
}

// Read only by the handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct OpenWorkspaceArgs {
    /// The workspace's root: a directory holding chant.workspace.json (an
    /// absolute path, or ~/...).
    pub dir: String,
    /// The environment whose gates and releases to read (default local).
    #[serde(default)]
    pub env: Option<String>,
    /// The user's chant principal, which approvals here record (github:LOGIN,
    /// or a signer's name). Default: their Arugula name, passed as is.
    #[serde(default)]
    pub actor: Option<String>,
    /// Editors' chant principals, by their Arugula name
    /// ({"sam": "github:sam-h"}), for gates an editor approves.
    #[serde(default)]
    pub principals: std::collections::BTreeMap<String, String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenPrArgs {
    /// The pull request: its link (a Forgejo PR's or a GitLab MR's),
    /// OWNER/REPO#N, GROUP/PROJECT!N (GitLab), or N (in dir's repository).
    pub pr: String,
    /// A clone of the repository on this host (absolute): what N means,
    /// and where diff and checkout fetch the PR's code.
    #[serde(default)]
    pub dir: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenIssueArgs {
    /// The issue: its link (GitHub's or Forgejo's), OWNER/REPO#N, or N (in
    /// dir's repository).
    pub issue: String,
    /// A clone of the repository on this host (absolute): what N means,
    /// and where the user's "Agent on this" makes its worktree.
    #[serde(default)]
    pub dir: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

/// A PR or issue block, to read.
#[derive(Deserialize, JsonSchema)]
pub struct ReadForgeArgs {
    /// The PR or issue block (from show).
    pub block: PaneArg,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommentArgs {
    /// The PR or issue block (from show).
    pub block: PaneArg,
    /// The comment, in markdown.
    pub body: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct IssueNewArgs {
    /// The repository: OWNER/REPO (default: dir's, or the one beside you),
    /// or its link (https://github.com/OWNER/REPO, a Forgejo one's), which
    /// says which forge.
    #[serde(default)]
    pub repo: Option<String>,
    /// A clone of it on this host (absolute).
    #[serde(default)]
    pub dir: Option<String>,
    /// The new issue's title.
    pub title: String,
    /// The issue's text, in markdown.
    #[serde(default)]
    pub body: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
    /// Your earlier draft's block, when the user asked for changes: the new title and text replace it there.
    #[serde(default)]
    pub block: Option<PaneArg>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum PrReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Deserialize, JsonSchema)]
pub struct PrReviewArgs {
    /// The PR block (from show).
    pub block: PaneArg,
    pub event: PrReviewEvent,
    /// The review's text (needed unless it approves).
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct PrMergeArgs {
    /// The PR block (from show).
    pub block: PaneArg,
    /// merge, rebase, rebase-merge, squash or fast-forward-only (default merge).
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct InvitePersonArgs {
    /// Whom: a name this machine knows (a teammate, someone the session is
    /// shared with), tailnet:<login> or account:<id>.
    pub who: String,
    /// viewer (the default) or editor.
    #[serde(default)]
    pub role: Option<String>,
    /// Where it opens, and so the session (default: your own pane; a full
    /// client outside a pane must say).
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// Why you want them there, at most 500 characters: the user reads it on
    /// the card, the person in their notification.
    pub note: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadInviteArgs {
    /// The draft id invite_person returned.
    pub draft: String,
    /// The pane you invited from, if you gave one then.
    #[serde(default)]
    pub pane: Option<PaneArg>,
}

// Read only by the delegate handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct DelegateArgs {
    /// The machine that offers the agent, by name (as list kind agents
    /// gives it) or id; this one's too. Leave it out to use the one that
    /// offers it (this machine first).
    #[serde(default)]
    pub machine: Option<String>,
    /// The agent, by name.
    pub agent: String,
    /// send: deliver the work as a pull request in the agent's own project,
    /// which it opens there (not as a patch for you to apply): for a fix
    /// in its project's code, not yours.
    #[serde(default)]
    pub pr: bool,
    /// send: the task; answer: the answer to the question it asked.
    #[serde(default)]
    pub text: Option<String>,
    /// get, answer, cancel: the task's id, as send returned it.
    #[serde(default)]
    pub task: Option<String>,
    /// How long to wait for it to finish, or to ask you something
    /// (seconds; default 300, at most 1800; 0: don't wait).
    #[serde(default)]
    pub wait: Option<u64>,
    /// review, apply: your checkout (default: where you work).
    #[serde(default)]
    pub dir: Option<String>,
}

/// `delegate`'s jobs.
const DELEGATE: &[Kind] = &[
    Kind {
        name: "send",
        description: "Hand a task to an agent offered on this machine or another (list kind agents lists them; machine can be left out): text is what to do, in that agent's project there, with what it needs to know (it doesn't see your conversation). Its owner may first have to allow it. Waits for it to finish (or ask you something) and returns its reply and the patch's stat; the patch itself is kept here. With pr: true it opens a pull request in its own project instead, and the reply links it.",
        schema: schema_for_type::<DelegateArgs>,
        flag: None,
    },
    Kind {
        name: "get",
        description: "A task's state now (with wait: once it finishes or asks): its status, reply and patch's stat.",
        schema: schema_for_type::<DelegateArgs>,
        flag: None,
    },
    Kind {
        name: "answer",
        description: "Answer the question a task asked (its state says input_required), or send a finished task a follow-up: text, then wait as send does.",
        schema: schema_for_type::<DelegateArgs>,
        flag: None,
    },
    Kind { name: "cancel", description: "Stop a task you sent.", schema: schema_for_type::<DelegateArgs>, flag: None },
    Kind {
        name: "review",
        description: "A finished task's patch, applied to a scratch copy of your checkout and shown in a diff block beside you, for the user to look at before anything changes here. Says whether it applies cleanly.",
        schema: schema_for_type::<DelegateArgs>,
        flag: None,
    },
    Kind {
        name: "apply",
        description: "Apply a finished task's patch in your checkout (git apply --3way): which files changed, conflicts to resolve, hunks that didn't apply, and new binary files the task left out.",
        schema: schema_for_type::<DelegateArgs>,
        flag: None,
    },
];

// Read only by the catalog's handler, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct ListTeamAgentsArgs {
    /// Words to find in an agent's name or description, or its machine's
    /// or owner's name.
    #[serde(default)]
    pub query: Option<String>,
    /// Ask every machine now, not from the last minute's read.
    #[serde(default)]
    pub fresh: bool,
}

// Read only by Managoat's handlers, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct ListAgentsArgs {
    /// Words to look for in each agent's name, description, skills and MCP
    /// servers (all must match): a skill's name finds the agents that have
    /// it.
    #[serde(default)]
    pub query: Option<String>,
    /// Where they come from: agent-specs (the curated ones), hand
    /// (hand-made) or app (made by an app). Default: all.
    #[serde(default)]
    pub source: Option<String>,
    /// The Managoat credentials profile (default: the user's default).
    #[serde(default)]
    pub profile: Option<String>,
}

// Read only by Managoat's handlers, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct ReadAgentArgs {
    /// The agent's name (or id), as kind managoat_agents gives it.
    pub name: String,
    /// The Managoat credentials profile (default: the user's default).
    #[serde(default)]
    pub profile: Option<String>,
}

// Read only by Managoat's handlers, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct ListFountainConversationsArgs {
    /// Only conversations carrying these labels, each "key:value" (all of
    /// them).
    #[serde(default)]
    pub labels: Vec<String>,
    /// Only these statuses, comma-separated: pending, running, idle, failed,
    /// terminated.
    #[serde(default)]
    pub status: Option<String>,
    /// At most this many, the most recently active first (1 to 500;
    /// default 100).
    #[serde(default)]
    pub limit: Option<usize>,
}

// Read only by Managoat's handlers, which a build without Labs lacks.
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
#[derive(Deserialize, JsonSchema)]
pub struct OpenFountainArgs {
    /// Start with this search (as kind managoat_agents' query).
    #[serde(default)]
    pub query: Option<String>,
    /// Start with this source: agent-specs, hand or app.
    #[serde(default)]
    pub source: Option<String>,
    /// The Managoat credentials profile (default: the user's default).
    #[serde(default)]
    pub profile: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<PaneArg>,
}

// ---------------------------------------------------------------- the list

type Schema = Arc<serde_json::Map<String, Value>>;

pub(crate) struct Def {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) args: Args,
    pub(crate) read_only: bool,
    pub(crate) destructive: bool,
    pub(crate) idempotent: bool,
    pub(crate) open_world: bool,
    /// The flag the machine needs on for it to be listed (`Op::FLAG`).
    pub(crate) flag: Option<&'static str>,
    /// Its answer's schema (`outputSchema`), when it is always one type: the
    /// answer's, with `summary`. A grouped tool answers by kind, and a tool
    /// that answers with more than one type in some case, has none.
    pub(crate) output: Option<fn() -> Schema>,
}

/// What a tool takes.
pub(crate) enum Args {
    /// One set of arguments, for a tool that does one thing.
    One(fn() -> Schema),
    /// A grouped tool (#349): its `kind` argument picks one of these jobs,
    /// and its schema is theirs merged. `default`: the kind when none is
    /// given (none: kind is required).
    Kinds { kinds: Vec<&'static Kind>, default: Option<&'static str> },
}

/// One job of a grouped tool, like `show {kind: "port", port: 3000}`. Each
/// is a row, so the kinds that belong to one feature (Managoat's, the
/// studio's) can be left out of their group, and their arguments with them,
/// by filtering its table ([`defs`]).
pub(crate) struct Kind {
    pub(crate) name: &'static str,
    /// What it does, for the tool's description.
    pub(crate) description: &'static str,
    pub(crate) schema: fn() -> Schema,
    /// The flag the machine needs on for it to be listed (`Op::FLAG`).
    pub(crate) flag: Option<&'static str>,
}

/// `show`'s blocks.
const SHOW: &[Kind] = &[
    Kind {
        name: "port",
        description: "A browser block on a port of the machine a pane runs on (a dev server), beside that pane, so the user sees it next to its terminal.",
        schema: schema_for_type::<OpenPortArgs>,
        flag: None,
    },
    Kind {
        name: "changes",
        description: "A diff block: what changed in a pane's git repository (the working tree against HEAD, or against rev_a, or rev_a..rev_b), as a file list with +/- the user can open to hunks and files, live while they look. Returns the files; read_output on the block gives the unified diff.",
        schema: schema_for_type::<ShowChangesArgs>,
        flag: None,
    },
    Kind {
        name: "file",
        description: "A file block: a file on a pane's machine, read-only, scrolled to a line and followed live, for the user to look at.",
        schema: schema_for_type::<ShowFileArgs>,
        flag: None,
    },
    Kind {
        name: "pr",
        description: "A pull request on the user's Forgejo or GitHub, or a GitLab merge request: its checks, reviews and timeline, read with the user's own tea, gh (or glab) login, and what it waits on them for (a review asked of them, red checks, changes requested) as attention on the phone and the swarm. Returns the PR as text; draft writes to it.",
        schema: schema_for_type::<OpenPrArgs>,
        flag: None,
    },
    Kind {
        name: "issue",
        description: "An issue on the user's Forgejo or GitHub: its labels, assignees, the pull requests that refer to it and its timeline, read with the user's own tea or gh login; one assigned to them or mentioning them is attention on the phone and the swarm. From the block the user can start an agent on it in a worktree of its own. Returns the issue as text.",
        schema: schema_for_type::<OpenIssueArgs>,
        flag: None,
    },
    super::ops::kind::<ConversationOpen>(),
    Kind {
        name: "app",
        description: "One of the user's studio apps (a box with its own agent, hud) as an app block: the app in a frame, and its agent's questions as asks on the block; send_input to the block prompts its agent. Without app: lists the user's apps.",
        schema: schema_for_type::<OpenAppArgs>,
        flag: None,
    },
    Kind {
        name: "workspace",
        description: "A chant workspace block: its members as cards (open a shell, an agent or the changes in one), its records, and the gates waiting for a person, which the user can approve there and which show as attention on the phone and the swarm. Read through the workspace's own chant. Returns its members and the gates waiting.",
        schema: schema_for_type::<OpenWorkspaceArgs>,
        flag: None,
    },
    Kind {
        name: "managoat",
        description: "The user's Managoat agents as a catalog block: a card per agent with its skills, servers and where it comes from, filters, and Run on Managoat / Spec for each. Returns the (filtered) list as text.",
        schema: schema_for_type::<OpenFountainArgs>,
        flag: None,
    },
];

/// `draft`'s writes to a forge.
const DRAFT: &[Kind] = &[
    Kind {
        name: "comment",
        description: "A comment on a PR or issue block's pull request or issue, on GitHub, Forgejo or GitLab.",
        schema: schema_for_type::<CommentArgs>,
        flag: None,
    },
    Kind {
        name: "review",
        description: "A review (approve, request_changes or comment) of a PR block's pull request.",
        schema: schema_for_type::<PrReviewArgs>,
        flag: None,
    },
    Kind {
        name: "merge",
        description: "Merging a PR block's pull request.",
        schema: schema_for_type::<PrMergeArgs>,
        flag: None,
    },
    Kind {
        name: "issue",
        description: "A new issue on GitHub or the user's Forgejo (repo as a link says which; a clone's remote does too): a block beside you holding the draft, for the user to send, edit, drop or ask you to change; it shows the issue once sent. Asked for changes, draft again with block set to that block. Returns the block.",
        schema: schema_for_type::<IssueNewArgs>,
        flag: None,
    },
];

/// `doc`'s jobs (#963): a document the user watches fill in.
const DOC: &[Kind] = &[
    Kind {
        name: "new",
        description: "A Doc: a block beside your pane that holds a report, plan or write-up, with its outline (every section's heading and intent) shown at once. Returns the block and the section ids (s1, s2, …); fill them next, in order.",
        schema: schema_for_type::<DocNewArgs>,
        flag: Some(flags::DOCS),
    },
    Kind {
        name: "fill",
        description: r##"Write one section of a Doc (its block, the section id, its Markdown): headings, paragraphs, lists, **bold**, *italic*, `code`, links and tables. No HTML. A picture is ![alt](assets/<name>) after doc asset has put it there. A section is at most 64 KB, a Doc 40 sections and 1 MiB; filling a section again replaces it.

Two kinds of fenced block draw as more than code, in the section that talks about them.
A chart: a ```chart fence holding a JSON spec. type is "bar" or "line"; title is optional; x is the key of each row that labels it; y is a key, or a list of up to 4 keys, whose values are numbers (not strings); rows is 1 to 200 objects. A line chart's x must go up: numbers, ISO dates (2026-01-31), or labels listed in order with "order": [...]. For example:
```chart
{"type": "bar", "title": "Sales by month", "x": "month", "y": ["sales"], "rows": [{"month": "Jan", "sales": 12}, {"month": "Feb", "sales": 19}]}
```
A drawing: a ```svg fence holding one <svg> element, with xmlns="http://www.w3.org/2000/svg" on it and a viewBox. Any text after svg on the fence line is its alt text. It is shown as a picture, so scripts in it don't run. For example:
```svg Idea to app
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 40"><rect x="2" y="8" width="60" height="24" fill="#e8f0fe" stroke="#335"/><path d="M62 20 H98" stroke="#335"/><rect x="98" y="8" width="60" height="24" fill="#e6f4ea" stroke="#335"/></svg>
```
Every fence is checked before anything is written: a bad chart or drawing is refused with one line saying what to fix, and the section stays as it was."##,
        schema: schema_for_type::<DocFillArgs>,
        flag: Some(flags::DOCS),
    },
    Kind {
        name: "asset",
        description: r##"Put a picture in a Doc (its block, a name, and one of path or data): a PNG, JPEG, GIF or WebP, at most 2 MB, at most 20 per Doc. path is an absolute path to a file you saved; an agent on a machine of its own sends data (base64) instead. What the bytes are decides the type, so the name has to end in the right extension (.png, .jpg, .jpeg, .gif, .webp); an SVG file is refused: draw it with a ```svg fence. Returns assets/<name>; show it in a section with ![what it shows](assets/<name>). Put the asset before the doc fill that names it: a fill naming an asset the Doc doesn't have is refused. Only assets/ images show; an http:// or ../ one is refused. A name already used is replaced. For example: {"kind": "asset", "block": 7, "name": "flow.png", "path": "/tmp/flow.png"}."##,
        schema: schema_for_type::<DocAssetArgs>,
        flag: Some(flags::DOCS),
    },
];

/// `list`'s lists.
const LIST: &[Kind] = &[
    super::ops::kind::<ListPanes>(),
    super::ops::kind::<ConversationsList>(),
    Kind {
        name: "links",
        description: "A conversation's lead only: the conversation's links (L1, L2…), each with what it is, where it is and its state now. Give a link's name as the pane of prompt_agent, wait and read_output.",
        schema: schema_for_type::<NoArgs>,
        flag: Some(flags::CHAT),
    },
    Kind {
        name: "devices",
        description: "The user's devices (their phone, say) that lend tools to agents: each with its tools and their arguments, and whether it's connected now. One that isn't can still be called with device_call: it's woken with a notification, which the user has to open.",
        schema: schema_for_type::<NoArgs>,
        flag: None,
    },
    Kind {
        name: "agents",
        description: "The agents the user's team offers: every Claude Code agent recipe offered on this machine, the user's other machines, their teams' and teammates', one line each: name, machine and owner, model, description. A machine that's offline shows its agents as last seen. To hand one a task (this machine's too), delegate {kind: send, agent: NAME}; for a fix in its own project, with pr: true. To run one of this machine's beside you, start_agent {recipe: NAME}.",
        schema: schema_for_type::<ListTeamAgentsArgs>,
        flag: None,
    },
    Kind {
        name: "managoat_agents",
        description: "The agents on the user's Managoat account, one compact row each: name, runtime and model, where it comes from (agent-specs: curated; hand: hand-made; app: made by an app), skills, MCP servers and description. query searches names, descriptions, skills and servers. To hand one a task, start_agent {agent: managoat, managoat_agent: NAME}.",
        schema: schema_for_type::<ListAgentsArgs>,
        flag: None,
    },
    Kind {
        name: "managoat_conversations",
        description: "The conversations on the user's Managoat account, read-only: every one, whoever started it (Arugula, the managoat CLI, a script, another conversation), the most recently active first, one line each: title, status, agent, unread, labels, what it started, and its open permission requests (waiting for the user, who answers them on their card). labels filters by key:value; status and limit as Managoat's list takes them.",
        schema: schema_for_type::<ListFountainConversationsArgs>,
        flag: None,
    },
    Kind {
        name: "managoat_agent",
        description: "One Managoat agent's whole recipe, by name: its system prompt, skills (inline or from GitHub), MCP servers, model, runtime, environment, sandbox provider and metadata, as Managoat returns it. Secrets are never in it: a server's credentials show as their ${VAR} references.",
        schema: schema_for_type::<ReadAgentArgs>,
        flag: None,
    },
];

/// Kinds listed to a conversation's lead alone (#858).
const LEAD_ONLY: [(&str, &str); 1] = [("list", "links")];

/// The jobs that work but aren't listed without their Labs flag: they're for
/// setups a stranger doesn't have (the studio, chant workspaces).
/// Each is a kind of a grouped tool, so without its flag it's filtered out
/// of its group's table, and its arguments and its line in the description
/// go with it. A caller who names one is refused, saying how to turn the
/// flag on (#347: the handler or `ops::flagged` asks), by its
/// kind or its name before #349.
const UNLISTED: [(&str, &str, &str); 3] =
    [("list", "agents", flags::AGENTS), ("show", "app", flags::STUDIO), ("show", "workspace", flags::WORKSPACES)];

/// Arguments of listed tools that aren't listed without their flag, by
/// tool: throwaway VMs and sandboxes, and recipes. A caller who gives one
/// is refused where the flag is off (a new machine, a recipe: #347), except
/// `machine`, which names one already made. Managoat's (`managoat_agent`,
/// `on`, `vault`, `environment`) left Labs with it (#942).
const UNLISTED_ARGS: [(&str, &[&str], &str); 3] = [
    ("run", &["vm", "vm_tab", "image", "machine"], flags::VMS),
    ("start_agent", &["vm"], flags::VMS),
    ("start_agent", &["recipe"], flags::AGENTS),
];

/// Managoat's kinds, arguments and values (#942): listed without a flag
/// since F5, and not at all by a build without Labs, which hasn't the code.
const FOUNTAIN_KINDS: [(&str, &str); 4] =
    [("show", "managoat"), ("list", "managoat_agents"), ("list", "managoat_agent"), ("list", "managoat_conversations")];
const FOUNTAIN_ARGS: [(&str, &[&str]); 1] = [("start_agent", &["managoat_agent", "on", "vault", "environment"])];
const FOUNTAIN_VALUES: [(&str, &str); 1] = [("start_agent", "managoat")];

/// Managoat's kinds by their names before the rename (DECISIONS.md
/// "Fountain is Managoat"): still answered, for an agent or a script that
/// kept them, and never listed. `start_agent`'s `agent: managoat`,
/// `fountain_agent` and `on: fountain` are read the same way (serde
/// aliases, and the match on `on` in `start_agent`).
fn old_kind(tool: &str, kind: &str) -> Option<&'static str> {
    match (tool, kind) {
        ("show", "fountain") => Some("managoat"),
        ("list", "fountain_agents") => Some("managoat_agents"),
        ("list", "fountain_agent") => Some("managoat_agent"),
        ("list", "fountain_conversations") => Some("managoat_conversations"),
        _ => None,
    }
}

/// A tool's schema without the arguments whose flag is off, and without
/// Managoat's in a build that hasn't it.
fn without_labs(tool: &str, schema: Schema, on: On) -> Schema {
    let fountain = crate::labs::DIRECT_MANAGOAT_RETIRED || !crate::labs::BUILT;
    let args: Vec<&str> = UNLISTED_ARGS
        .iter()
        .filter(|(t, _, f)| *t == tool && !on.has(f))
        .flat_map(|(_, a, _)| a.iter().copied())
        .chain(FOUNTAIN_ARGS.iter().filter(|(t, _)| fountain && *t == tool).flat_map(|(_, a)| a.iter().copied()))
        .collect();
    let values: Vec<&str> = FOUNTAIN_VALUES.iter().filter(|(t, _)| fountain && *t == tool).map(|(_, v)| *v).collect();
    if args.is_empty() && values.is_empty() {
        return schema;
    }
    let mut s = (*schema).clone();
    if let Some(props) = s.get_mut("properties").and_then(Value::as_object_mut) {
        props.retain(|name, _| !args.contains(&name.as_str()));
    }
    if let Some(needed) = s.get_mut("required").and_then(Value::as_array_mut) {
        needed.retain(|n| !n.as_str().is_some_and(|n| args.contains(&n)));
    }
    for v in s.values_mut() {
        drop_values(v, &values);
    }
    Arc::new(s)
}

/// Takes `values` out of every enum in a schema, whether schemars wrote it
/// as `enum` or as `oneOf` a `const` each.
fn drop_values(schema: &mut Value, values: &[&str]) {
    let gone = |x: &Value| x.as_str().is_some_and(|x| values.contains(&x));
    match schema {
        Value::Object(o) => {
            for key in ["oneOf", "anyOf"] {
                if let Some(alts) = o.get_mut(key).and_then(Value::as_array_mut) {
                    alts.retain(|a| {
                        let only = a
                            .get("enum")
                            .and_then(Value::as_array)
                            .is_some_and(|e| !e.is_empty() && e.iter().all(gone));
                        !(a.get("const").is_some_and(gone) || only)
                    });
                }
            }
            if let Some(e) = o.get_mut("enum").and_then(Value::as_array_mut) {
                e.retain(|x| !gone(x));
            }
            for v in o.values_mut() {
                drop_values(v, values);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|v| drop_values(v, values)),
        _ => {}
    }
}

/// The tools of the forge: a build without it (`forge` feature) lists none.
const FORGE_TOOLS: [&str; 2] = ["read_forge", "draft"];

/// Whether `tool`'s kind is the forge's (`show` a PR or an issue), which a
/// build without the forge doesn't list.
fn forge_kind(tool: &str, kind: &str) -> bool {
    tool == "show" && matches!(kind, "pr" | "issue")
}

/// Whether a grouped tool's kind is listed with the flags in `on`: its own
/// (`Op::FLAG`, for an operation's), or the table's.
fn listed_kind(on: On, tool: &str, kind: &Kind) -> bool {
    kind.flag.is_none_or(|f| on.has(f))
        && UNLISTED.iter().all(|(t, k, f)| (*t, *k) != (tool, kind.name) || on.has(f))
        && (crate::forges::BUILT || !forge_kind(tool, kind.name))
        && ((!crate::labs::DIRECT_MANAGOAT_RETIRED && crate::labs::BUILT)
            || !FOUNTAIN_KINDS.contains(&(tool, kind.name)))
}

/// The tools `tools/list` shows: each one a stranger can't use needs its
/// flag, and so does each kind they can't use in the rest.
fn defs(on: On) -> Vec<Def> {
    all_defs()
        .into_iter()
        .filter(|d| d.flag.is_none_or(|f| on.has(f)))
        .filter(|d| crate::forges::BUILT || !FORGE_TOOLS.contains(&d.name))
        // M78: delegating is the agent catalog's.
        .filter(|d| on.has(flags::AGENTS) || d.name != "delegate")
        .map(|mut d| {
            let name = d.name;
            if let Args::Kinds { kinds, .. } = &mut d.args {
                kinds.retain(|k| listed_kind(on, name, k));
            }
            d
        })
        .collect()
}

/// The blocks `capabilities` offers with the flags in `on` (#229): a
/// terminal and an agent block, and each kind `show` lists, so a kind a
/// flag leaves out of `show` is left out here too.
pub(crate) fn block_offers(on: On) -> Vec<results::BlockOffer> {
    let first = |s: &str| s.split_once(". ").map_or(s, |(a, _)| a).trim_end_matches('.').to_owned() + ".";
    let mut out = vec![
        results::BlockOffer {
            kind: "terminal".into(),
            open: "run".into(),
            about: "A command in a terminal pane the user can watch and take over.".into(),
        },
        results::BlockOffer {
            kind: "agent".into(),
            open: "start_agent".into(),
            about: "An agent (Claude Code, Codex, any ACP agent) whose approvals and questions come to its block."
                .into(),
        },
    ];
    let show = defs(on).into_iter().find(|d| d.name == "show");
    if let Some(Def { args: Args::Kinds { kinds, .. }, .. }) = show {
        out.extend(kinds.iter().map(|k| results::BlockOffer {
            kind: k.name.into(),
            open: format!("show {{kind: \"{}\"}}", k.name),
            about: first(k.description),
        }));
    }
    // #963: a Doc opens with its own tool, `new`.
    if on.has(flags::DOCS) {
        out.push(results::BlockOffer {
            kind: "doc".into(),
            open: "doc {kind: \"new\"}".into(),
            about: "A report, plan or write-up the user watches fill in, section by section.".into(),
        });
    }
    out
}

/// A group's table as a tool holds it.
fn table(kinds: &'static [Kind]) -> Vec<&'static Kind> {
    kinds.iter().collect()
}

/// Every tool there is, listed or not, with every kind.
fn all_defs() -> Vec<Def> {
    vec![
        Def {
            name: "run",
            title: "Run a command in a pane",
            description: "Run a command in a new terminal pane (a new tab, or a split). The command is typed into a shell, so the user can watch it, scroll it and take over, and it outlives this conversation. With wait, waits for it to finish (up to timeout) and returns its exit code and last lines; past the timeout it answers \"still running\": call wait.",
            args: Args::One(schema_for_type::<RunArgs>),
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
            flag: None,
            output: Some(output_schema::<results::RunState>),
        },
        Def {
            name: "send_input",
            title: "Type into a pane",
            description: "Type text (Enter after it unless enter is false) and/or press named keys (C-c, Up, Escape, ...) in a pane. To an agent block, text is its next prompt.",
            args: Args::One(schema_for_type::<SendArgs>),
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "attach",
            title: "Attach a file to a pane",
            description: "Put a file (a screenshot, an image, a log) into a pane: into a terminal, its path is pasted where a shell or an agent like claude reads it (whatever else is in front is refused unless force); to an agent block, it goes with text as its next prompt, an image as an image. Give a path on this host, or the file as base64 data.",
            args: Args::One(schema_for_type::<AttachArgs>),
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "read_output",
            title: "Read a pane's output",
            description: "A pane's output as text (escape sequences stripped): the latest, from an offset, or its last command's. Paged: pass next_offset back as offset for more. With screen: what it shows right now (for full-screen programs).",
            args: Args::One(schema_for_type::<ReadArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "wait",
            title: "Wait for a pane",
            description: "Wait until a pane's command ends, its process exits, its output matches a pattern, or it's idle or asks for input (an agent's approval or question). Answers \"still running\" with the offset after timeout seconds (default 100): call it again.",
            args: Args::One(schema_for_type::<WaitArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "delegate",
            title: "Delegate to a teammate's agent",
            description: "A task for an agent offered on this machine or another (your own, your team's, a teammate's), over A2A: send it, follow it, answer its questions, or cancel it.",
            args: Args::Kinds { kinds: table(DELEGATE), default: None },
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
            flag: None,
            output: None,
        },
        Def {
            name: "list",
            title: "List panes and more",
            description: "What's here to work with, by kind (default panes).",
            args: Args::Kinds { kinds: table(LIST), default: Some("panes") },
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: true,
            flag: None,
            output: None,
        },
        Def {
            name: "capabilities",
            title: "What Arugula offers here",
            description: "What this machine's Arugula offers you, to plan your work around: the blocks you can show the user (and the call that opens each), your tools, the Labs flags on, which forges the user is signed in to here (PR and issue blocks, and the drafts they send), whether Claude Code's hooks are live in your pane (your questions and permission prompts as cards on the phone and the swarm, follow-ups through arugula inbox), and whether this machine has joined Arugula control and which teams it knows.",
            args: Args::One(schema_for_type::<NoArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: Some(output_schema::<results::Capabilities>),
        },
        super::ops::def::<ClosePane>(),
        super::ops::def::<ThreadGet>(),
        super::ops::def::<ThreadPost>(),
        Def {
            name: "history",
            title: "Command history and output",
            description: "What happened across panes (open and recently closed), newest first: commands with exit codes, directories, when, and who ran them, and answers and approvals with who gave them. Each has a kind (command, answer, agent or action: a forge action or an app prompt); filter by kind, failed (commands only), since/before (\"2d\", \"36h\"), cwd, a regex. kind output instead: lines of output across panes (and agents' transcripts) matching pattern, a regex, with the pane and command each came from (since and limit apply).",
            args: Args::One(schema_for_type::<HistoryArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "show",
            title: "Show the user a block",
            description: "Show the user something as a block beside a pane (beside; default the caller's own pane), instead of describing it. kind says what.",
            args: Args::Kinds { kinds: table(SHOW), default: None },
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
            flag: None,
            output: None,
        },
        Def {
            name: "doc",
            title: "Build a document for the user",
            description: "Build a Doc, a report, plan or write-up longer than a screen, in a block beside your pane that the user (and anyone the session is shared with) watches fill in. Make the outline first with new, so they see every section at once; then fill each section in order and say one sentence about it in the pane. Never write the Doc's files by hand.",
            args: Args::Kinds { kinds: table(DOC), default: None },
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
            flag: Some(flags::DOCS),
            output: None,
        },
        Def {
            name: "start_agent",
            title: "Start an agent",
            description: "Start an agent (Claude Code, Codex, any ACP agent) in an agent block with a prompt. Its approvals and questions come to the block; wait until needs_input, then agent_respond, or leave them for the user.",
            args: Args::One(schema_for_type::<StartAgentArgs>),
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
            flag: None,
            output: Some(output_schema::<results::BlockOnly>),
        },
        Def {
            name: "prompt_agent",
            title: "Prompt an agent and wait",
            description: "Give an agent (an agent block, or Claude Code or Codex in a terminal) a prompt and wait for its turn in one call: returns when the turn ends (done), when it asks for someone (needs_input, with the question: agent_respond answers it), or stalled with its screen's last lines if it shows no sign of work within a few seconds (no agent there, the prompt not submitted, the agent gone). An agent already waiting on an approval or question isn't typed at; its question comes back (pass answering to type the answer).",
            args: Args::One(schema_for_type::<PromptArgs>),
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "agent_respond",
            title: "Answer an agent",
            description: "Allow or deny an agent's pending permission request, or answer or skip its pending question (as wait until needs_input showed it).",
            args: Args::One(schema_for_type::<RespondArgs>),
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
            flag: None,
            output: Some(output_schema::<results::PaneOnly>),
        },
        Def {
            name: "read_forge",
            title: "Read a PR or issue block",
            description: "A PR or issue block's pull request or issue as text (header, body, checks, reviews, linked pull requests, timeline), what it waits on the user for, an issue's agent and its PR, and your drafts: each waiting, sent (with its link and who sent it) or dropped (and by whom).",
            args: Args::One(schema_for_type::<ReadForgeArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: true,
            flag: None,
            output: Some(output_schema::<results::ForgeRead>),
        },
        Def {
            name: "draft",
            title: "Draft a forge write",
            description: "Draft a write to a pull request or issue, by kind. Nothing reaches the forge in an agent's name: the draft waits on its block as a card the user (or an editor) edits and sends, or drops. Returns its draft id (a new issue: its block) at once; read_forge shows what became of it.",
            args: Args::Kinds { kinds: table(DRAFT), default: None },
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
            flag: None,
            output: None,
        },
        Def {
            name: "invite_person",
            title: "Ask to invite a person",
            description: "Ask the user to bring someone into the session you work in (your pane's): a teammate, someone it's shared with, or tailnet:<login>, as a viewer (default) or an editor, with a note saying why. Nothing is shared in an agent's name: it waits as a card beside you that only the session's owner sends (after editing the role or note, if they like) or declines. Only the owner's own agents may ask (not one a guest started). Returns its draft id at once with status waiting; read_invite shows what became of it.",
            args: Args::One(schema_for_type::<InvitePersonArgs>),
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
            flag: None,
            output: Some(output_schema::<results::InviteDrafted>),
        },
        Def {
            name: "read_invite",
            title: "Read an invite draft",
            description: "What became of an invite_person draft: waiting, sent (with its grant and delivery: sent, pending or unreachable, and why), declined (and the owner's reason), dropped (nobody answered in a day) or failed (and why); who settled it and when.",
            args: Args::One(schema_for_type::<ReadInviteArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: None,
        },
        Def {
            name: "read_file",
            title: "Read a file",
            description: "A text file on this host or the machine a pane runs on, paged by byte offset.",
            args: Args::One(schema_for_type::<ReadFileArgs>),
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
            flag: None,
            output: Some(output_schema::<results::FileRead>),
        },
        Def {
            name: "device_call",
            title: "Use a tool on the user's device",
            description: "Call a tool on one of the user's devices (list kind devices shows them): take a photo, where the phone is, and so on. The user sees each call on the device and allows or denies it there, so it can take a while; it waits up to timeout seconds (default 100). A photo comes back as a file path on this host. If the device isn't connected, it's woken with a notification first.",
            args: Args::One(schema_for_type::<DeviceCallArgs>),
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
            flag: None,
            output: Some(output_schema::<results::DeviceCalled>),
        },
    ]
}

/// What a tool's name meant before #349 grouped them: the tool now, and
/// the argument that picks the old job. Answered for a client that kept
/// the old names (a script, a list cached before an upgrade); never listed.
fn renamed(old: &str) -> Option<(&'static str, Option<(&'static str, Value)>)> {
    let kind = |tool: &'static str, k: &str| Some((tool, Some(("kind", json!(k)))));
    match old {
        "capture_screen" => Some(("read_output", Some(("screen", json!(true))))),
        "search" => kind("history", "output"),
        "list_conversations" => kind("list", "conversations"),
        "list_devices" => kind("list", "devices"),
        "list_agents" => kind("list", "managoat_agents"),
        "read_agent" => kind("list", "managoat_agent"),
        "open_port" => kind("show", "port"),
        "show_changes" => kind("show", "changes"),
        "show_file" => kind("show", "file"),
        "open_pr" => kind("show", "pr"),
        "open_issue" => kind("show", "issue"),
        "open_conversation" => kind("show", "conversation"),
        "open_app" => kind("show", "app"),
        "open_workspace" => kind("show", "workspace"),
        "open_fountain" => kind("show", "managoat"),
        "read_pr" | "read_issue" => Some(("read_forge", None)),
        "pr_comment" | "issue_comment" => kind("draft", "comment"),
        "pr_review" => kind("draft", "review"),
        "pr_merge" => kind("draft", "merge"),
        "issue_new" => kind("draft", "issue"),
        _ => None,
    }
}

/// A grouped tool's schema: `kind`, then every kind's arguments. One that
/// several kinds take has one entry, its description saying which kind
/// means what; what each kind needs is in the tool's description
/// ([`kinds_text`]), since a flat schema can't say it.
fn grouped_schema(kinds: &[&Kind], default: Option<&str>) -> Schema {
    let mut kind = json!({
        "type": "string",
        "enum": kinds.iter().map(|k| k.name).collect::<Vec<_>>(),
        "description": "Which one (see the tool's description).",
    });
    if let Some(d) = default {
        kind["default"] = json!(d);
    }
    let mut props = serde_json::Map::new();
    props.insert("kind".into(), kind);
    let mut defs = serde_json::Map::new();
    // Each argument's descriptions, with the kinds that give each one, in
    // the order the kinds name them.
    let mut said: Vec<(String, Said)> = Vec::new();
    for k in kinds {
        let s = (k.schema)();
        for (name, p) in s.get("properties").and_then(Value::as_object).into_iter().flatten() {
            let mut p = p.clone();
            let what = p.as_object_mut().and_then(|o| o.remove("description"));
            let what = what.and_then(|w| w.as_str().map(str::to_owned)).unwrap_or_default();
            match props.get(name) {
                None => {
                    props.insert(name.clone(), p);
                }
                // The same argument, optional for one kind and needed by
                // another: the schema that allows null.
                Some(was) if was != &p && nullable(&p) => {
                    props.insert(name.clone(), p);
                }
                Some(_) => {}
            }
            let at = match said.iter().position(|(n, _)| n == name) {
                Some(i) => i,
                None => {
                    said.push((name.clone(), Vec::new()));
                    said.len() - 1
                }
            };
            match said[at].1.iter_mut().find(|(w, _)| *w == what) {
                Some((_, ks)) => ks.push(k.name),
                None => said[at].1.push((what, vec![k.name])),
            }
        }
        for (name, d) in s.get("$defs").and_then(Value::as_object).into_iter().flatten() {
            defs.entry(name.clone()).or_insert_with(|| d.clone());
        }
    }
    for (name, whats) in said {
        let whats: Vec<(String, Vec<&str>)> = whats.into_iter().filter(|(w, _)| !w.is_empty()).collect();
        let text = match whats.as_slice() {
            [] => continue,
            [(w, ks)] if ks.len() == kinds.len() => w.clone(),
            _ => whats.iter().map(|(w, ks)| format!("{}: {w}", ks.join(", "))).collect::<Vec<_>>().join(" "),
        };
        if let Some(o) = props.get_mut(&name).and_then(Value::as_object_mut) {
            o.insert("description".into(), json!(text));
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("$schema".into(), json!("https://json-schema.org/draft/2020-12/schema"));
    out.insert("type".into(), json!("object"));
    out.insert("properties".into(), Value::Object(props));
    if default.is_none() {
        out.insert("required".into(), json!(["kind"]));
    }
    if !defs.is_empty() {
        out.insert("$defs".into(), Value::Object(defs));
    }
    Arc::new(out)
}

/// An argument's descriptions in a group, each with the kinds that give it.
type Said<'a> = Vec<(String, Vec<&'a str>)>;

/// Whether an argument's schema allows null (an optional one).
fn nullable(p: &Value) -> bool {
    let null = |t: &Value| t == "null" || t.as_array().is_some_and(|a| a.iter().any(|t| t == "null"));
    p.get("type").is_some_and(null)
        || ["anyOf", "oneOf"].iter().any(|k| {
            p.get(*k).and_then(Value::as_array).is_some_and(|a| a.iter().any(|s| s.get("type").is_some_and(null)))
        })
}

/// A kind's arguments as its line in the tool's description shows them,
/// the ones it needs first: `port, beside?, path?`.
fn kind_args(k: &Kind) -> Vec<String> {
    let s = (k.schema)();
    let needed: Vec<&str> =
        s.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let (mut first, mut then) = (Vec::new(), Vec::new());
    for (n, _) in s.get("properties").and_then(Value::as_object).into_iter().flatten() {
        match needed.contains(&n.as_str()) {
            true => first.push(n.clone()),
            false => then.push(format!("{n}?")),
        }
    }
    first.extend(then);
    first
}

/// A grouped tool's description: its own, then a line per kind with what
/// it takes.
fn kinds_text(lead: &str, kinds: &[&Kind], default: Option<&str>) -> String {
    let mut text = lead.to_owned();
    for k in kinds {
        let args = kind_args(k);
        let dflt = if default == Some(k.name) { " (the default)" } else { "" };
        let args = if args.is_empty() { String::new() } else { format!(" {{{}}}", args.join(", ")) };
        text.push_str(&format!("\n- {}{dflt}{args}: {}", k.name, k.description));
    }
    text
}

/// No arguments.
#[derive(Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
pub struct DeviceCallArgs {
    /// The tool's name, from list kind devices.
    pub tool: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Option<Value>,
    /// Which device: its id or part of its name. Optional when there's one.
    #[serde(default)]
    pub device: Option<String>,
    /// Seconds to wait for the answer, including waking the device
    /// (default 100, at most 3600).
    #[serde(default)]
    pub timeout: Option<f64>,
}

/// The tools `scope` may call, with the flags in `on`.
pub fn list(scope: Scope, on: On) -> Vec<Tool> {
    defs(on)
        .into_iter()
        .filter(|d| scope != Scope::Read || d.read_only)
        .map(|mut d| {
            // A lead's links (#858) are its own.
            if let Args::Kinds { kinds, .. } = &mut d.args
                && scope.leads().is_none()
            {
                kinds.retain(|k| !LEAD_ONLY.contains(&(d.name, k.name)));
            }
            d
        })
        .map(|d| {
            let (description, schema) = match &d.args {
                Args::One(schema) => (d.description.to_owned(), without_labs(d.name, schema(), on)),
                Args::Kinds { kinds, default } => {
                    (kinds_text(d.description, kinds, *default), grouped_schema(kinds, *default))
                }
            };
            let tool = Tool::new(d.name, description, schema).with_title(d.title).with_annotations(
                ToolAnnotations::with_title(d.title)
                    .read_only(d.read_only)
                    .destructive(d.destructive)
                    .idempotent(d.idempotent)
                    .open_world(d.open_world),
            );
            match d.output {
                Some(output) => tool.with_raw_output_schema(output()),
                None => tool,
            }
        })
        .collect()
}

/// What a call reaches: its tool (by its name now, for an old one), the
/// kind it picks for a grouped one, and its arguments without the kind.
/// Every kind answers, listed or not; `on` only says which ones an error
/// names.
fn route(name: &str, mut args: Value, on: On) -> Result<(Def, Option<&'static str>, Value), String> {
    let name = match renamed(name) {
        Some((tool, set)) => {
            if let (Some((key, v)), Some(o)) = (set, args.as_object_mut()) {
                o.insert(key.into(), v);
            }
            tool
        }
        None => name,
    };
    let def = all_defs().into_iter().find(|d| d.name == name).ok_or_else(|| format!("no tool {name}"))?;
    let kind = match &def.args {
        Args::One(_) => None,
        Args::Kinds { kinds, default } => {
            let shown: Vec<&str> = kinds.iter().filter(|k| listed_kind(on, def.name, k)).map(|k| k.name).collect();
            Some(pick_kind(def.name, kinds, &shown, *default, &mut args)?)
        }
    };
    Ok((def, kind, args))
}

/// A grouped tool's call: the kind it names (or the default), taken out
/// of `args`, which must then be arguments that kind takes. `shown`: the
/// kinds an error names.
fn pick_kind(
    tool: &str,
    kinds: &[&Kind],
    shown: &[&str],
    default: Option<&'static str>,
    args: &mut Value,
) -> Result<&'static str, String> {
    let names = || shown.join(", ");
    let given = args.as_object_mut().and_then(|o| o.remove("kind"));
    let name = match &given {
        Some(Value::String(s)) => s.as_str(),
        Some(v) => return Err(format!("{tool}'s kind is a string, one of {}; not {v}", names())),
        None => default.ok_or_else(|| format!("{tool} needs a kind: one of {}", names()))?,
    };
    let name = old_kind(tool, name).unwrap_or(name);
    let k = kinds
        .iter()
        .find(|k| k.name == name)
        .ok_or_else(|| format!("{tool} has no kind {name:?}: it's one of {}", names()))?;
    let takes = kind_args(k);
    for key in args.as_object().into_iter().flatten().map(|(n, _)| n) {
        if !takes.iter().any(|t| t.trim_end_matches('?') == key) {
            let takes = match takes.is_empty() {
                true => "it takes nothing else".to_owned(),
                false => format!("it takes {}", takes.join(", ")),
            };
            return Err(format!("{key} isn't an argument of {tool} kind {}: {takes}", k.name));
        }
    }
    Ok(k.name)
}

// ---------------------------------------------------------------- calls

pub(crate) type Out = Result<Value, String>;

/// The `outputSchema` of a tool whose successful answer is always a `T`: the
/// schema of `T` as `done` sends it, which is the object with `summary` in it
/// (a required string). The root is `type: object` (an enum's `oneOf` gets
/// it here), since MCP clients may check `structuredContent` against it.
pub(crate) fn output_schema<T: JsonSchema + 'static>() -> Schema {
    let mut s = (*schema_for_type::<T>()).clone();
    // The wrapper type's name and doc are noise to an agent.
    s.remove("title");
    s.remove("description");
    s.entry("type").or_insert_with(|| Value::String("object".into()));
    let props = s.entry("properties").or_insert_with(|| Value::Object(Default::default()));
    props["summary"] = json!({ "type": "string" });
    let required = s.entry("required").or_insert_with(|| Value::Array(vec![]));
    required.as_array_mut().expect("required is a list").push(Value::String("summary".into()));
    let mut s = Value::Object(s);
    strip_formats(&mut s);
    let Value::Object(s) = s else { unreachable!() };
    Arc::new(s)
}

/// The formats JSON Schema 2020-12 defines; schemars' others (`uint32`,
/// `int64`, `double`…) are Rust's, and clients warn about each one.
const STANDARD_FORMATS: &[&str] = &[
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uuid",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// Drops every `format` that isn't a standard one, anywhere in the schema.
fn strip_formats(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if o.get("format").and_then(Value::as_str).is_some_and(|f| !STANDARD_FORMATS.contains(&f)) {
                o.remove("format");
            }
            o.values_mut().for_each(strip_formats);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_formats),
        _ => {}
    }
}

/// A result, with the `summary` sentence in it.
pub(crate) fn done(summary: impl Into<String>, v: impl Serialize) -> Out {
    let mut v = serde_json::to_value(v).map_err(|e| e.to_string())?;
    v["summary"] = Value::String(summary.into());
    Ok(v)
}

pub(crate) fn parse<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("bad arguments: {e}"))
}

/// One tool call (or resource read): who, and how to tell them it's still
/// going.
pub struct Call<'a> {
    app: &'a Arc<App>,
    caller: Caller,
    /// The client's name (`claude-code`), for "started by mcp:<client>".
    client: String,
    peer: Peer<RoleServer>,
    progress: Option<ProgressToken>,
    ctx: RequestContext<RoleServer>,
    /// A conversation's lead (#858) naming a link here: the link and its
    /// pane, which this call alone may reach.
    link: std::sync::OnceLock<(String, PaneId)>,
}

impl<'a> Call<'a> {
    pub fn new(app: &'a Arc<App>, caller: Caller, client: String, ctx: &RequestContext<RoleServer>) -> Self {
        Self {
            app,
            caller,
            client,
            peer: ctx.peer.clone(),
            progress: ctx.meta.get_progress_token(),
            ctx: ctx.clone(),
            link: std::sync::OnceLock::new(),
        }
    }

    /// This call is on link `name` (#858), pane `pane` here.
    #[cfg_attr(not(feature = "labs"), allow(dead_code, reason = "a lead's links are Labs'"))]
    pub(crate) fn set_link(&self, name: &str, pane: PaneId) {
        let _ = self.link.set((name.to_owned(), pane));
    }

    /// The pane of the link this call is on, if it's one here.
    fn link_pane(&self) -> Option<PaneId> {
        self.link.get().map(|(_, p)| *p)
    }

    pub(crate) fn app(&self) -> &'a Arc<App> {
        self.app
    }

    pub(crate) fn by(&self) -> String {
        format!("mcp:{}", self.client)
    }

    /// The client's name (`claude-code`).
    pub(crate) fn client(&self) -> &str {
        &self.client
    }

    fn driver(&self) -> Driver {
        Driver { who: self.by(), name: self.by() }
    }

    pub async fn dispatch(&self, name: &str, args: Value) -> CallToolResult {
        let fail = |e: String| CallToolResult::error(vec![ContentBlock::text(e)]);
        let on = flags::on(self.app.control.state_dir());
        let (def, kind, args) = match route(name, args, on) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let name = def.name;
        if self.caller.scope == Scope::Read && !def.read_only {
            return fail(format!("this token may only read; {name} changes things (ask for a full token)"));
        }
        // #858: a lead names its links: one here is its pane for this call;
        // one elsewhere is done there.
        let lead = self.caller.scope.leads();
        let args = match lead {
            Some(conv) => match crate::labs::lead::route(self, conv, name, args).await {
                Ok(args) => args,
                Err(Ok(v)) => return CallToolResult::structured(v),
                Err(Err(e)) => return fail(e),
            },
            None => args,
        };
        let mut out = match (name, kind) {
            ("run", _) => match parse(args) {
                Ok(a) => self.run(a).await,
                Err(e) => Err(e),
            },
            ("send_input", _) => match parse(args) {
                Ok(a) => self.send_input(a).await,
                Err(e) => Err(e),
            },
            ("attach", _) => match parse(args) {
                Ok(a) => self.attach(a).await,
                Err(e) => Err(e),
            },
            ("read_output", _) => match parse(args) {
                Ok(a) => self.read_output(a).await,
                Err(e) => Err(e),
            },
            ("device_call", _) => match parse(args) {
                Ok(a) => self.device_call(a).await,
                Err(e) => Err(e),
            },
            ("wait", _) => match parse(args) {
                Ok(a) => self.wait(a).await,
                Err(e) => Err(e),
            },
            ("close", _) => super::ops::call::<ClosePane>(self, args).await,
            ("read_thread", _) => super::ops::call::<ThreadGet>(self, args).await,
            ("post_thread", _) => super::ops::call::<ThreadPost>(self, args).await,
            ("history", _) => match parse(args) {
                Ok(a) => self.history_or_output(a).await,
                Err(e) => Err(e),
            },
            // M44's as_fountain (a Managoat agent worn here) was retired:
            // said, not ignored into a plain Claude Code.
            ("start_agent", _) if args.get("as_fountain").is_some_and(|v| !v.is_null()) => {
                Err(crate::agent::defs::AS_FOUNTAIN_GONE.into())
            }
            ("start_agent", _) => match parse(args) {
                Ok(a) => self.start_agent(a).await,
                Err(e) => Err(e),
            },
            ("prompt_agent", _) => match parse(args) {
                Ok(a) => self.prompt_agent(a).await,
                Err(e) => Err(e),
            },
            ("agent_respond", _) => match parse(args) {
                Ok(a) => self.respond(a).await,
                Err(e) => Err(e),
            },
            ("read_file", _) => match parse(args) {
                Ok(a) => self.read_file(a).await,
                Err(e) => Err(e),
            },
            ("read_forge", _) => match parse(args) {
                Ok(a) => self.read_forge(a).await,
                Err(e) => Err(e),
            },
            ("invite_person", _) => match parse(args) {
                Ok(a) => self.invite_person(a).await,
                Err(e) => Err(e),
            },
            ("read_invite", _) => match parse(args) {
                Ok(a) => self.read_invite(a).await,
                Err(e) => Err(e),
            },
            ("capabilities", _) => match parse::<NoArgs>(args) {
                Ok(_) => self.capabilities().await,
                Err(e) => Err(e),
            },
            // list
            ("list", Some("panes")) => super::ops::call::<ListPanes>(self, args).await,
            ("list", Some("conversations")) => super::ops::call::<ConversationsList>(self, args).await,
            ("list", Some("devices")) => self.list_devices(),
            ("list", Some("links")) => crate::labs::lead::links_now(self).await,
            ("delegate", Some(kind @ ("send" | "get" | "answer" | "cancel" | "review" | "apply"))) => match parse(args)
            {
                Ok(a) => self.delegate(kind, a).await,
                Err(e) => Err(e),
            },
            ("list", Some("agents")) => match parse(args) {
                Ok(a) => self.team_agents(a).await,
                Err(e) => Err(e),
            },
            ("list", Some("managoat_agents")) => match parse(args) {
                Ok(a) => self.list_agents(a).await,
                Err(e) => Err(e),
            },
            ("list", Some("managoat_agent")) => match parse(args) {
                Ok(a) => self.read_agent(a).await,
                Err(e) => Err(e),
            },
            ("list", Some("managoat_conversations")) => match parse(args) {
                Ok(a) => self.fountain_conversations(a).await,
                Err(e) => Err(e),
            },
            ("doc", Some("new")) => match parse(args) {
                Ok(a) => self.doc_new(a).await,
                Err(e) => Err(e),
            },
            ("doc", Some("fill")) => match parse(args) {
                Ok(a) => self.doc_fill(a).await,
                Err(e) => Err(e),
            },
            ("doc", Some("asset")) => match parse(args) {
                Ok(a) => self.doc_asset(a).await,
                Err(e) => Err(e),
            },
            // show
            ("show", Some("port")) => match parse(args) {
                Ok(a) => self.open_port(a).await,
                Err(e) => Err(e),
            },
            ("show", Some("changes")) => match parse(args) {
                Ok(a) => self.show_changes(a).await,
                Err(e) => Err(e),
            },
            ("show", Some("file")) => match parse(args) {
                Ok(a) => self.show_file(a).await,
                Err(e) => Err(e),
            },
            ("show", Some("pr")) => match parse::<OpenPrArgs>(args) {
                Ok(a) => self.open_forge(ForgeOpen { pr: Some(a.pr), ..Default::default() }, a.dir, a.beside).await,
                Err(e) => Err(e),
            },
            ("show", Some("issue")) => match parse::<OpenIssueArgs>(args) {
                Ok(a) => {
                    self.open_forge(ForgeOpen { issue: Some(a.issue), ..Default::default() }, a.dir, a.beside).await
                }
                Err(e) => Err(e),
            },
            ("show", Some("conversation")) => super::ops::call::<ConversationOpen>(self, args).await,
            ("show", Some("app")) => match parse(args) {
                Ok(a) => self.open_app(a).await,
                Err(e) => Err(e),
            },
            ("show", Some("workspace")) => match parse(args) {
                Ok(a) => self.open_workspace(a).await,
                Err(e) => Err(e),
            },
            ("show", Some("managoat")) => match parse(args) {
                Ok(a) => self.open_fountain(a).await,
                Err(e) => Err(e),
            },
            // draft
            ("draft", Some("comment")) => match parse::<CommentArgs>(args) {
                Ok(a) => self.pr_write(&a.block, Write::Comment { body: a.body }).await,
                Err(e) => Err(e),
            },
            ("draft", Some("review")) => match parse::<PrReviewArgs>(args) {
                Ok(a) => {
                    let event = match a.event {
                        PrReviewEvent::Approve => ReviewEvent::Approve,
                        PrReviewEvent::RequestChanges => ReviewEvent::RequestChanges,
                        PrReviewEvent::Comment => ReviewEvent::Comment,
                    };
                    self.pr_write(&a.block, Write::Review { event, body: a.body }).await
                }
                Err(e) => Err(e),
            },
            ("draft", Some("merge")) => match parse::<PrMergeArgs>(args) {
                Ok(a) => self.pr_write(&a.block, Write::Merge { style: a.style }).await,
                Err(e) => Err(e),
            },
            ("draft", Some("issue")) => match parse::<IssueNewArgs>(args) {
                Ok(IssueNewArgs { block: Some(block), title, body, .. }) => {
                    self.redraft_issue(&block, title, body.unwrap_or_default()).await
                }
                Ok(a) => {
                    let c = ForgeOpen {
                        issue: Some("new".into()),
                        title: Some(a.title),
                        body: Some(a.body.unwrap_or_default()),
                        repo: a.repo,
                        by: Some(self.by()),
                        agent: Some(true),
                        pane: self.own_pane(),
                        ..Default::default()
                    };
                    self.open_forge(c, a.dir, a.beside).await
                }
                Err(e) => Err(e),
            },
            (_, Some(k)) => Err(format!("{name} kind {k} isn't wired up")),
            (_, None) => Err(format!("no tool {name}")),
        };
        if let (Some(conv), Some((link, pane)), Ok(v)) = (lead, self.link.get(), &mut out) {
            // Its name, not its number, which reaches nothing from a lead.
            if let Some(s) = v["summary"].as_str() {
                v["summary"] = json!(s.replace(&format!("%{pane}"), link));
            }
            v["link"] = json!(link);
            crate::labs::lead::after(self.app, conv, link, name, v).await;
        }
        match out {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => fail(e),
        }
    }

    fn list_devices(&self) -> Out {
        let devices = self.app.hands.list();
        let summary = match devices.len() {
            0 => "No device lends tools here yet.".to_owned(),
            n => format!(
                "{n} device(s): {}",
                devices.iter().map(|d| d["name"].as_str().unwrap_or("?")).collect::<Vec<_>>().join(", ")
            ),
        };
        done(summary, results::Devices { devices })
    }

    /// `history`: its entries, or with kind output, lines of output
    /// (search before #349). Each takes its own arguments.
    async fn history_or_output(&self, a: HistoryArgs) -> Out {
        if a.kind.as_deref() != Some("output") {
            if a.pattern.is_some() {
                return Err("pattern goes with kind output (lines of output); match filters command lines".into());
            }
            return self.history(a).await;
        }
        let others = [
            ("failed", a.failed),
            ("before", a.before.is_some()),
            ("cwd", a.cwd.is_some()),
            ("match", a.matching.is_some()),
            ("pane", a.pane.is_some()),
        ];
        if let Some((key, _)) = others.iter().find(|(_, set)| *set) {
            return Err(format!("{key} isn't an argument of history kind output: it takes pattern, since?, limit?"));
        }
        let pattern = a.pattern.ok_or("history kind output needs a pattern: a regex for the lines to find")?;
        self.search(SearchArgs { pattern, since: a.since, limit: a.limit }).await
    }

    // ------------------------------------------------------------ helpers

    async fn panes(&self) -> Vec<PaneSummary> {
        self.app.mux.api(Api::Panes).await.unwrap_or_default()
    }

    /// Whether this caller is an agent on a machine (a guest's, say): what
    /// it creates must land on a machine too ([`confine`]).
    pub(crate) async fn on_machine(&self) -> Result<bool, String> {
        match self.me() {
            Some(me) => Ok(self.readable(me).await?.info.host.is_some()),
            None => Ok(false),
        }
    }

    /// The block whose token this is.
    async fn device_call(&self, a: DeviceCallArgs) -> Out {
        let from = format!("{} on {}", self.client, self.app.hosts.name());
        let call = self.app.hands.call(
            &self.app.control,
            a.device.as_deref(),
            &a.tool,
            a.args.unwrap_or_else(|| json!({})),
            &from,
        );
        let limit = Self::limit(a.timeout);
        let Some(r) = self.waiting(&format!("waiting for the device ({})", a.tool), limit, call).await else {
            return Err(format!("no answer from the device within {}s", limit.as_secs()));
        };
        let c = r?;
        let woke = c.woke_ms.map_or(String::new(), |ms| format!(" (woken: back in {:.1}s)", ms as f64 / 1000.0));
        done(
            format!("{} answered {}{woke}", c.name, a.tool),
            results::DeviceCalled {
                device: c.device,
                name: c.name,
                result: c.result,
                woke_ms: c.woke_ms,
                answer_ms: c.answer_ms,
            },
        )
    }

    /// Where a tool defaults to when it's given no pane: the caller's own
    /// (an agent block, or the terminal pane `arugula mcp` runs in).
    pub(crate) fn own_pane(&self) -> Option<PaneId> {
        self.caller.pane
    }

    /// `run`'s `split`: a pane, or "self" for the caller's own.
    fn split_pane(&self, split: Option<&PaneArg>) -> Result<Option<PaneId>, String> {
        match split {
            Some(PaneArg::Name(s)) if s.trim().eq_ignore_ascii_case("self") => self.own_pane().map(Some).ok_or_else(|| {
                "split \"self\": this caller has no pane of its own (run arugula mcp inside an Arugula pane, or give a pane number)".into()
            }),
            other => other.map(PaneArg::id).transpose(),
        }
    }

    /// #379: the `CLAUDE_CONFIG_DIR` the stdio bridge sent for its client.
    fn claude_config_dir(&self) -> Option<String> {
        let parts = self.ctx.extensions.get::<axum::http::request::Parts>()?;
        let d = parts.headers.get("arugula-claude-config-dir")?.to_str().ok()?.trim();
        (!d.is_empty()).then(|| d.to_owned())
    }

    /// What the caller may reach.
    pub(crate) fn scope(&self) -> Scope {
        self.caller.scope
    }

    /// The agent block whose token this is: what confines a caller.
    pub(crate) fn me(&self) -> Option<PaneId> {
        self.caller.scope.block()
    }

    /// An agent in a VM of its own (#59) works on that machine: the first
    /// time it starts something there, its machine becomes its tab's (as
    /// "Share machine with tab" does), so what it starts can join it and
    /// the machine stays while they do. Nothing for other callers, or when
    /// the machine isn't the agent's own.
    async fn share_my_machine(&self) {
        let Some(me) = self.me() else { return };
        let _ = self.app.mux.api(|r| Api::ShareMachine(me, r)).await;
    }

    /// An open pane or block, if this caller may see it.
    pub(crate) async fn readable(&self, pane: PaneId) -> Result<PaneSummary, String> {
        let panes = self.panes().await;
        let Some(p) = panes.iter().find(|p| p.info.id == pane).cloned() else {
            return Err(self.gone(pane).await);
        };
        // A lead's link here, named for this call (#858).
        if self.link_pane() == Some(pane) {
            return Ok(p);
        }
        if let Some(me) = self.me() {
            let tab = panes.iter().find(|p| p.info.id == me).map(|p| p.tab);
            if tab != Some(p.tab) {
                if self.caller.scope.leads().is_some() {
                    return Err(format!(
                        "%{pane} isn't in your tab: a lead reaches its links by name (L1, L2…; list kind links), never by number"
                    ));
                }
                return Err(format!("%{pane} is in another tab: this agent's token reaches its own tab only"));
            }
        }
        Ok(p)
    }

    /// An open pane or block this caller may type in, answer for or close:
    /// anything, or for an agent block's token, what it started.
    pub(crate) async fn drivable(&self, pane: PaneId) -> Result<PaneSummary, String> {
        let p = self.readable(pane).await?;
        if let Some(me) = self.me()
            && self.link_pane() != Some(pane)
            && p.info.started_by.as_ref().and_then(|s| s.block) != Some(me)
        {
            return Err(format!("%{pane} wasn't started by this agent: it may read it, not drive or close it"));
        }
        Ok(p)
    }

    /// Why a pane isn't there, as specifically as history can say.
    pub(crate) async fn gone(&self, pane: PaneId) -> String {
        let store = self.app.mux.store.clone();
        let last = tokio::task::spawn_blocking(move || {
            history::history(
                &store,
                &Filter { pane: Some(pane), kind: Some(HistoryKind::Command), ..Default::default() },
                1,
            )
        })
        .await
        .ok()
        .and_then(|h| h.into_iter().next());
        match last {
            Some(h) => {
                let what = h.text.as_deref().map(|t| format!(" `{}`", one_line(t, 80))).unwrap_or_default();
                let how = match h.exit {
                    Some(code) => format!("exited {code}"),
                    None => "was still running".into(),
                };
                let when = ago(h.ended_ms.unwrap_or(h.started_ms));
                format!("pane %{pane} is gone; its last command{what} {how} {when} (read_output still reads it)")
            }
            None => format!("no pane %{pane} (list shows what's open)"),
        }
    }

    async fn started(&self, pane: PaneId) {
        let by = StartedBy { by: self.by(), block: self.me() };
        self.app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(pane, by)));
    }

    async fn notify(&self, done: f64, total: Option<f64>, message: String) {
        let Some(token) = &self.progress else { return };
        let mut p = ProgressNotificationParam::new(token.clone(), done).with_message(message);
        if let Some(t) = total {
            p = p.with_total(t);
        }
        let _ = self.peer.notify_progress(p).await;
    }

    /// `fut`, with progress every 15s, for at most `limit`: `None` if it
    /// took longer (or the client gave up).
    pub(crate) async fn waiting<T>(&self, what: &str, limit: Duration, fut: impl Future<Output = T>) -> Option<T> {
        let start = Instant::now();
        let every = progress_every();
        let mut tick = tokio::time::interval_at(start + every, every);
        let deadline = tokio::time::sleep(limit);
        tokio::pin!(fut, deadline);
        loop {
            tokio::select! {
                r = &mut fut => return Some(r),
                _ = &mut deadline => return None,
                _ = self.ctx.ct.cancelled() => return None,
                _ = tick.tick() => {
                    let secs = start.elapsed().as_secs_f64();
                    self.notify(secs, Some(limit.as_secs_f64()), format!("{what}: {}s", secs.round())).await;
                }
            }
        }
    }

    pub(crate) fn limit(timeout: Option<f64>) -> Duration {
        timeout
            .filter(|t| t.is_finite() && *t >= 0.0)
            .map(Duration::from_secs_f64)
            .unwrap_or(WAIT_DEFAULT)
            .min(WAIT_MAX)
    }

    /// The end of a pane's output stream now.
    async fn end_of(&self, pane: PaneId) -> u64 {
        match self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten() {
            Some(p) => p.status().end,
            None => PaneLog::open(self.app.mux.store.pane_dir(pane)).map(|l| l.end()).unwrap_or(0),
        }
    }

    /// The last lines of a pane's output between two offsets.
    fn last_lines(&self, pane: PaneId, from: u64, to: Option<u64>) -> String {
        let Ok(log) = PaneLog::open(self.app.mux.store.pane_dir(pane)) else { return String::new() };
        let to = to.unwrap_or(log.end());
        let from = from.max(to.saturating_sub(64 * 1024));
        let Ok((start, bytes)) = log.read_range(from, (to.saturating_sub(from)) as usize) else { return String::new() };
        let _ = start;
        let text = strip(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
        cap_tail(&tail, 4_000)
    }

    // ------------------------------------------------------------ tools

    async fn run(&self, a: RunArgs) -> Out {
        if a.command.as_deref().is_some_and(|c| c.trim().is_empty()) {
            return Err("the command is empty".into());
        }
        let policy = match a.policy.as_deref() {
            None | Some("shell") => None,
            Some("none") => Some(Policy::None),
            Some("rerun") => Some(Policy::Rerun { confirm: false }),
            Some("rerun-ask") => Some(Policy::Rerun { confirm: true }),
            Some(p) => return Err(format!("policy {p}: shell, none, rerun or rerun-ask")),
        };
        let mut req = RunRequest { cwd: a.cwd.clone(), policy, ..Default::default() };
        match self.me() {
            Some(me) => {
                if a.vm || a.vm_tab || a.machine.is_some() || a.session.is_some() {
                    return Err("this agent's token reaches its own tab only: run splits a pane there (no vm, vm_tab, machine or session)".into());
                }
                let split = self.split_pane(a.split.as_ref())?.unwrap_or(me);
                let p = self.readable(split).await?;
                if p.info.host.is_some() {
                    self.share_my_machine().await;
                }
                req.split = Some(split);
                // On the tab's machine, in a VM tab.
                req.join = p.info.host.is_some();
                req.from_pane = Some(split);
            }
            None => {
                req.split = self.split_pane(a.split.as_ref())?;
                req.join = a.join;
                req.vm = a.vm;
                req.vm_tab = a.vm_tab;
                req.image = a.image.clone();
                req.sandbox = a.machine.clone();
                req.session = a.session.clone();
                req.from_pane = req.split;
            }
        }
        confine(self.on_machine().await?, run_lands_on_machine(&req))?;
        let on_vm = a.vm || a.vm_tab || a.machine.is_some() || req.join;
        let pane = match self.app.mux.api(|r| Api::Run(req, r)).await {
            Some(Ok(p)) => p,
            Some(Err(e)) => return Err(e),
            None => return Err("the daemon is shutting down".into()),
        };
        self.started(pane).await;
        let limit = Self::limit(a.timeout);
        let start = Instant::now();
        let place = self.place(pane).await;
        let Some(command) = a.command else {
            return done(format!("Opened a shell in %{pane} ({place})"), RunState::Started { pane });
        };
        // The shell's first prompt (a VM takes a while), then the command.
        let mux = self.app.mux.clone();
        let ready = self
            .waiting(
                if on_vm { "starting the machine" } else { "starting the shell" },
                if on_vm { Duration::from_secs(300) } else { Duration::from_secs(20) },
                async move {
                    loop {
                        if let Some(p) = mux.api(|r| Api::Pane(pane, r)).await.flatten() {
                            let st = p.status();
                            if st.at_prompt || st.exited.is_some() {
                                return st.exited;
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            )
            .await;
        if let Some(Some(code)) = ready {
            return Err(format!("%{pane}'s shell ended (exit {code:?}) before the command could be typed"));
        }
        let Some(p) = self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten() else {
            return Err(self.gone(pane).await);
        };
        // Whatever ends after this is the command's.
        p.mark_input();
        let mut line = command.clone().into_bytes();
        line.push(b'\r');
        self.app.mux.send(crate::mux::Cmd::Api(Api::InputBy(pane, line, self.by())));
        if !a.wait {
            return done(
                format!(
                    "Started `{}` in %{pane} ({place}). Follow it with wait (until command_end) or read_output.",
                    one_line(&command, 80)
                ),
                RunState::Started { pane },
            );
        }
        let left = limit.saturating_sub(start.elapsed());
        self.wait_command(pane, left, &command).await
    }

    /// Where a pane is, in words.
    async fn place(&self, pane: PaneId) -> String {
        let panes = self.panes().await;
        match panes.iter().find(|p| p.info.id == pane) {
            Some(p) => {
                let tab = p.tab_name.clone().unwrap_or_else(|| format!("tab {}", p.tab));
                match p.info.host {
                    Some(m) => format!("{tab} in {}, on machine m{m}", p.session_name),
                    None => format!("{tab} in {}", p.session_name),
                }
            }
            None => "closing".into(),
        }
    }

    async fn wait_command(&self, pane: PaneId, limit: Duration, what: &str) -> Out {
        let app = self.app.clone();
        let r = self
            .waiting(&format!("`{}` running", one_line(what, 40)), limit, async move {
                crate::api::wait_until(&app, pane, "command-end", None).await
            })
            .await;
        match r {
            Some(Ok(r)) => self.waited(pane, r).await,
            Some(Err(e)) => Err(e),
            None => self.still_running(pane, limit).await,
        }
    }

    /// Nothing yet: what it's doing, and where to pick up. `until`: what
    /// was waited for, when it isn't a command ending.
    async fn still_running(&self, pane: PaneId, waited: Duration) -> Out {
        self.not_yet(pane, waited, None).await
    }

    async fn not_yet(&self, pane: PaneId, waited: Duration, until: Option<&str>) -> Out {
        let p = self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten();
        let status = p.map(|p| p.status());
        let current = status.as_ref().and_then(|s| s.current.clone());
        let end = self.end_of(pane).await;
        let from = current.as_ref().map(|c| c.start).unwrap_or(end.saturating_sub(4096));
        let what = current
            .as_ref()
            .and_then(|c| c.text.clone())
            .map(|t| format!(" `{}`", one_line(&t, 60)))
            .unwrap_or_default();
        let secs = waited.as_secs();
        let then = format!("call wait again (or read_output with offset {end} for what's new)");
        let summary = match (until, current.is_some()) {
            (None, true) => format!("%{pane} is still running{what} after {secs}s: {then}"),
            (u, _) => format!("%{pane}: no {} yet after {secs}s: {then}", u.unwrap_or("command end")),
        };
        done(
            summary,
            RunState::StillRunning {
                pane,
                command: current.and_then(|c| c.text),
                next_offset: end,
                last_lines: self.last_lines(pane, from, None),
            },
        )
    }

    async fn waited(&self, pane: PaneId, r: WaitResult) -> Out {
        match r {
            WaitResult::CommandEnd { text, exit, start, end } => {
                let what =
                    text.as_deref().map(|t| format!("`{}`", one_line(t, 60))).unwrap_or_else(|| "the command".into());
                let how = match exit {
                    Some(0) => "succeeded".to_owned(),
                    Some(c) => format!("failed (exit {c})"),
                    None => "ended".to_owned(),
                };
                let next = match end {
                    Some(e) => e,
                    None => self.end_of(pane).await,
                };
                done(
                    format!("{what} in %{pane} {how}; read_output with last_command for all of its output"),
                    RunState::Done {
                        pane,
                        command: text,
                        exit,
                        output_offset: start,
                        next_offset: next,
                        last_lines: self.last_lines(pane, start, end),
                    },
                )
            }
            WaitResult::Exit { code } => done(
                format!("%{pane}'s process exited{}", code.map(|c| format!(" {c}")).unwrap_or_default()),
                RunState::Exited { pane, exit: code, last_lines: self.last_lines(pane, 0, None) },
            ),
            WaitResult::Match { text, offset } => done(
                format!("%{pane}'s output matched: {}", one_line(&text, 80)),
                RunState::Matched { pane, matched: text, offset, next_offset: self.end_of(pane).await },
            ),
            WaitResult::Attention { state, ask } => {
                let summary = match (&state, &ask) {
                    (Attention::NeedsInput, Some(a)) => {
                        format!("%{pane} asks: {}. Answer with agent_respond (or leave it for the user)", a.headline())
                    }
                    (Attention::NeedsInput, None) => format!("%{pane} needs input"),
                    (Attention::Done, _) => format!("%{pane} finished"),
                    _ => format!("%{pane} is idle"),
                };
                done(summary, results::Attended { pane, state, ask })
            }
            WaitResult::Timeout => self.still_running(pane, Duration::ZERO).await,
        }
    }

    /// M71: a file into a terminal (M70's upload and paste) or an agent
    /// block (its prompt's file), under the checks the routes make.
    #[cfg(unix)]
    async fn attach(&self, a: AttachArgs) -> Out {
        use base64::Engine;
        const MAX: u64 = 20 << 20;
        let pane = a.pane.id()?;
        let p = self.drivable(pane).await?;
        if !matches!(p.info.kind, BlockType::Terminal | BlockType::Agent) {
            return Err("attach puts a file into a terminal or an agent block".into());
        }
        let (bytes, ext) = match (a.path, a.data) {
            (Some(path), None) => {
                // Its paths are on its machine, not here.
                if let Some(me) = self.me()
                    && self.app.mux.api(|r| Api::MachineOf(me, r)).await.flatten().is_some()
                {
                    return Err("this agent runs on a machine of its own: send the file as data".into());
                }
                let size = tokio::fs::metadata(&path).await.map_err(|e| format!("{path}: {e}"))?.len();
                if size > MAX {
                    return Err("a file can be 20 MB at most".into());
                }
                let bytes = tokio::fs::read(&path).await.map_err(|e| format!("{path}: {e}"))?;
                let ext = std::path::Path::new(&path).extension().map(|e| e.to_string_lossy().into_owned());
                (bytes, ext)
            }
            (None, Some(data)) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data.trim())
                    .map_err(|e| format!("data isn't base64: {e}"))?;
                (bytes, a.ext)
            }
            _ => return Err("give path or data".into()),
        };
        let ext = match crate::agent::images::sniff(&bytes) {
            Some(m) => m.trim_start_matches("image/").replace("jpeg", "jpg"),
            None => ext.unwrap_or_default(),
        };
        let path = crate::upload::store_whole(self.app, pane, &ext, bytes).await.map_err(|e| e.1)?;
        let by = self.by();
        if p.info.kind == BlockType::Agent {
            let b =
                self.app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            b.call_by("send", json!({ "text": a.text.unwrap_or_default(), "files": [path] }), Some(&by)).await?;
            return done(format!("Sent %{pane} a prompt with the file"), results::PaneOnly { pane });
        }
        let out =
            crate::upload::paste_into(self.app, pane, vec![path.clone()], a.force, Some(&by)).await.map_err(|e| e.1)?;
        if out["pasted"] != true {
            let front = out["front"].as_str().unwrap_or("something");
            return Err(format!(
                "%{pane} is running {front}, which wouldn't read a path; it's at {path} (force pastes it anyway)"
            ));
        }
        done(format!("Pasted {path} into %{pane}"), results::Pasted { pane, path })
    }

    #[cfg(not(unix))]
    async fn attach(&self, a: AttachArgs) -> Out {
        let _ = (a.pane, a.path, a.data, a.ext, a.text, a.force);
        Err("files can't be attached on this host yet".into())
    }

    async fn send_input(&self, a: SendArgs) -> Out {
        let pane = a.pane.id()?;
        let p = self.drivable(pane).await?;
        if a.text.is_none() && a.keys.is_empty() {
            return Err("give text, keys, or both".into());
        }
        if p.info.kind != BlockType::Terminal {
            let b =
                self.app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            let text = a.text.ok_or("an agent block takes text (its next prompt), not keys")?;
            if p.info.kind == BlockType::App {
                let out = b.call_by("send", json!({ "text": text, "tab": a.tab }), Some(&self.by())).await?;
                let tab = out["tab"].as_str().unwrap_or("its tab");
                let queued = match out["position"].as_u64() {
                    Some(n) if n > 0 => format!(", queued {n} behind a running turn"),
                    _ => String::new(),
                };
                return done(
                    format!("Prompted %{pane}'s agent in {tab}{queued}; wait until needs_input for its questions"),
                    results::Prompted {
                        pane,
                        tab: tab.to_owned(),
                        chat: out["chat"].clone(),
                        position: out["position"].clone(),
                    },
                );
            }
            b.call_by("send", json!({ "text": text }), Some(&self.by())).await?;
            return done(format!("Sent %{pane} a prompt"), results::PaneOnly { pane });
        }
        let handle =
            self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten().ok_or_else(|| format!("no pane %{pane}"))?;
        let mut data = a.text.clone().unwrap_or_default().into_bytes();
        if a.text.is_some() && a.enter.unwrap_or(true) {
            data.push(b'\r');
        }
        let modes = handle.status().modes;
        for k in &a.keys {
            let bytes = crate::keys::key(k, modes);
            if bytes.is_empty() {
                return Err(format!("unknown key {k:?}: try C-c, Up, Enter, Escape, Tab, F5, M-x"));
            }
            data.extend(bytes);
        }
        handle.mark_input();
        let offset = handle.status().end;
        self.app.mux.send(crate::mux::Cmd::Api(Api::InputBy(pane, data, self.by())));
        done(
            format!("Typed into %{pane}; its output from here starts at offset {offset}"),
            results::Typed { pane, next_offset: offset },
        )
    }

    async fn read_output(&self, a: ReadArgs) -> Out {
        if a.screen {
            return self.capture(PaneOnly { pane: a.pane }).await;
        }
        let pane = a.pane.id()?;
        let max = a.max_chars.unwrap_or(PAGE).clamp(200, PAGE_MAX);
        // Open, or closed and still on disk (a full token only: a closed
        // pane's tab is unknown).
        let info = match self.readable(pane).await {
            Ok(p) => Some(p),
            Err(e) if self.me().is_some() => return Err(e),
            Err(_) => None,
        };
        if let Some(p) = &info
            && p.info.kind != BlockType::Terminal
        {
            let b =
                self.app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            return page_text(pane, &b.text(), a.offset, max);
        }
        let closed = || {
            self.app
                .mux
                .store
                .pane_dirs()
                .into_iter()
                .find(|(p, _, _)| *p == pane)
                .map(|(_, _, d)| d)
                .ok_or_else(|| format!("no pane %{pane}, open or closed"))
        };
        let status = self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten().map(|p| p.status());
        // A pane closing as this runs has its directory moved to closed/
        // between the lookup and the open: read it from there.
        let log = match info {
            Some(_) => match PaneLog::open(self.app.mux.store.pane_dir(pane)) {
                Ok(log) => Ok(log),
                Err(_) => PaneLog::open(closed()?),
            },
            None => PaneLog::open(closed()?),
        }
        .map_err(|e| format!("can't read %{pane}'s output: {e}"))?;
        let mut command = None;
        let (from, until) = if a.last_command {
            let st =
                status.as_ref().ok_or("a closed pane's commands: use history, then read_output with its offset")?;
            let c = st.current.clone().or(st.last.clone()).ok_or("no command recorded in that pane yet")?;
            command =
                Some(results::OutputCommand { text: c.text.clone(), exit: c.exit, running: st.current.is_some() });
            (Some(a.offset.unwrap_or(c.start)), if st.current.is_some() { None } else { c.end })
        } else {
            (a.offset, None)
        };
        let end = until.unwrap_or(log.end()).min(log.end());
        let (start, text, next) = match from {
            Some(from) => {
                let from = from.clamp(log.start(), end);
                let (_, raw) = log
                    .read_range(from, ((end - from) as usize).min(max * 4).min(256 * 1024))
                    .map_err(|e| format!("can't read %{pane}'s output: {e}"))?;
                let (text, used) = head_page(&raw, max);
                (from, text, from + used as u64)
            }
            None => {
                let from = end.saturating_sub((max * 4).min(256 * 1024) as u64).max(log.start());
                let (_, raw) = log
                    .read_range(from, (end - from) as usize)
                    .map_err(|e| format!("can't read %{pane}'s output: {e}"))?;
                let (text, skipped) = tail_page(&raw, max);
                (from + skipped as u64, text, end)
            }
        };
        let more = next < end;
        let summary = if more {
            format!("%{pane}: {} characters from offset {start}; more: call again with offset {next}", text.len())
        } else if until.is_some() {
            format!("%{pane}: {} characters, to the end of its last command", text.len())
        } else {
            format!("%{pane}: {} characters, up to now (offset {next}); later output starts there", text.len())
        };
        done(summary, results::Output { pane, offset: start, next_offset: next, more, command, text })
    }

    async fn capture(&self, a: PaneOnly) -> Out {
        let pane = a.pane.id()?;
        let p = self.readable(pane).await?;
        let text = if p.info.kind == BlockType::Terminal {
            let h =
                self.app.mux.api(|r| Api::Pane(pane, r)).await.flatten().ok_or_else(|| format!("no pane %{pane}"))?;
            tokio::task::spawn_blocking(move || h.capture(CaptureFormat::Text, CaptureScope::Screen))
                .await
                .ok()
                .flatten()
                .ok_or("the pane didn't answer")?
        } else {
            let b =
                self.app.mux.api(|r| Api::Block(pane, r)).await.flatten().ok_or_else(|| format!("no block %{pane}"))?;
            b.text()
        };
        let text = cap_tail(text.trim_end(), PAGE_MAX);
        done(format!("%{pane}'s screen ({} lines)", text.lines().count()), results::Screen { pane, text })
    }

    async fn wait(&self, a: WaitArgs) -> Out {
        let pane = a.pane.id()?;
        let p = self.readable(pane).await?;
        let until = match a.until {
            Until::CommandEnd => "command-end",
            Until::Exit => "exit",
            Until::Match => "match",
            Until::Idle => "idle",
            Until::NeedsInput => "needs-input",
        };
        if p.info.kind != BlockType::Terminal && !matches!(until, "idle" | "needs-input") {
            return Err(format!("%{pane} is an agent or browser block: wait until idle or needs_input"));
        }
        if until == "match" && a.pattern.is_none() {
            return Err("until match needs a pattern".into());
        }
        let limit = Self::limit(a.timeout);
        let app = self.app.clone();
        let pattern = a.pattern.clone();
        let what = match until {
            "needs-input" => format!("waiting for %{pane} to ask"),
            "idle" => format!("waiting for %{pane} to finish"),
            _ => format!("waiting for %{pane}"),
        };
        let r =
            self.waiting(&what, limit, async move { crate::api::wait_until(&app, pane, until, pattern).await }).await;
        match r {
            Some(Ok(r)) => self.waited(pane, r).await,
            Some(Err(e)) if e.contains("closed") => Err(self.gone(pane).await),
            Some(Err(e)) => Err(e),
            None => {
                let label = match until {
                    "command-end" => None,
                    "exit" => Some("exit"),
                    "match" => Some("match"),
                    "idle" => Some("end of its turn"),
                    _ => Some("question"),
                };
                self.not_yet(pane, limit, label).await
            }
        }
    }

    /// Panes this caller may read the history of: all, or its tab's.
    async fn tab_panes(&self) -> Option<Vec<PaneId>> {
        let me = self.me()?;
        let panes = self.panes().await;
        let tab = panes.iter().find(|p| p.info.id == me).map(|p| p.tab);
        Some(panes.iter().filter(|p| Some(p.tab) == tab).map(|p| p.info.id).collect())
    }

    /// The thread a call names: a pane's or a session's, within what this
    /// caller reaches (an agent block's token: its own tab and session).
    pub(crate) async fn thread_of(
        &self,
        pane: Option<&PaneArg>,
        session: Option<SessionId>,
    ) -> Result<ThreadTarget, String> {
        // A lead's thread is its conversation (#858), and only that.
        if let Some(conv) = self.caller.scope.leads() {
            if pane.is_some() || session.is_some() {
                return Err("a lead reads and posts in its conversation only: give no pane or session".into());
            }
            return Ok(ThreadTarget::Conv(conv));
        }
        match (pane.map(PaneArg::id).transpose()?, session) {
            (Some(_), Some(_)) => Err("give a pane or a session, not both".into()),
            (Some(p), None) => self.readable(p).await.map(|_| ThreadTarget::Pane(p)),
            (None, Some(s)) => {
                if let Some(me) = self.me()
                    && self.readable(me).await?.session != s
                {
                    return Err(format!("session {s} isn't this agent's: its token reaches its own session only"));
                }
                Ok(ThreadTarget::Session(s))
            }
            (None, None) => match self.own_pane() {
                Some(own) => self.readable(own).await.map(|_| ThreadTarget::Pane(own)),
                None => Err("which thread? give a pane or a session".into()),
            },
        }
    }

    async fn history(&self, a: HistoryArgs) -> Out {
        let limit = a.limit.unwrap_or(30).clamp(1, 200);
        let since = a.since.as_deref().map(seconds).transpose()?;
        let before = a.before.as_deref().map(seconds).transpose()?;
        let matching = a.matching.as_deref().map(regex::Regex::new).transpose().map_err(|e| format!("match: {e}"))?;
        let pane = a.pane.as_ref().map(PaneArg::id).transpose()?;
        let only = self.tab_panes().await;
        let kind = a
            .kind
            .as_deref()
            .map(|k| {
                HistoryKind::parse(k).ok_or_else(|| {
                    format!("kind: {k:?} isn't command, answer, agent or action (or output, for lines of output)")
                })
            })
            .transpose()?;
        let filter = Filter {
            pane,
            failed: a.failed,
            kind,
            since_ms: since.map(|s| now_ms().saturating_sub(s * 1000)),
            cwd: a.cwd.clone(),
            matching,
        };
        let store = self.app.mux.store.clone();
        let all =
            tokio::task::spawn_blocking(move || history::history(&store, &filter, 100_000)).await.unwrap_or_default();
        let cut = before.map(|b| now_ms().saturating_sub(b * 1000));
        let mut hits: Vec<HistoryEntry> = all
            .into_iter()
            .filter(|h| cut.is_none_or(|c| h.started_ms <= c))
            .filter(|h| only.as_ref().is_none_or(|o| o.contains(&h.pane)))
            .collect();
        let total = hits.len();
        hits.reverse();
        hits.truncate(limit);
        let entries: Vec<results::CommandEntry> = hits
            .iter()
            .map(|h| results::CommandEntry {
                pane: h.pane,
                open: h.open,
                command: h.text.clone(),
                cwd: h.cwd.clone(),
                exit: h.exit,
                started_ms: h.started_ms,
                started: ago(h.started_ms),
                seconds: h.ended_ms.map(|e| e.saturating_sub(h.started_ms) / 1000),
                by: h.by.clone(),
                kind: h.kind,
                output_offset: h.start,
            })
            .collect();
        let summary = match (total, a.failed) {
            (0, true) => "No failed commands match".to_owned(),
            (0, false) => "No commands match".to_owned(),
            (n, f) => format!(
                "{n} {}command{} match; the newest {} here (read_output with pane and output_offset for one's output)",
                if f { "failed " } else { "" },
                if n == 1 { "" } else { "s" },
                entries.len()
            ),
        };
        done(summary, results::Commands { commands: entries, total })
    }

    async fn search(&self, a: SearchArgs) -> Out {
        let re = regex::Regex::new(&a.pattern).map_err(|e| format!("pattern: {e}"))?;
        let limit = a.limit.unwrap_or(30).clamp(1, 200);
        let since = a.since.as_deref().map(seconds).transpose()?.map(|s| now_ms().saturating_sub(s * 1000));
        let only = self.tab_panes().await;
        let store = self.app.mux.store.clone();
        let kinds = self.app.mux.kinds.clone();
        let want = if only.is_some() { 2000 } else { limit };
        let hits = tokio::task::spawn_blocking(move || history::search(&store, &kinds, &re, since, want))
            .await
            .unwrap_or_default();
        let hits: Vec<results::Hit> = hits
            .into_iter()
            .filter(|h| only.as_ref().is_none_or(|o| o.contains(&h.pane)))
            .take(limit)
            .map(|h| results::Hit {
                pane: h.pane,
                open: h.open,
                offset: h.offset,
                command: h.command,
                thread: h.thread,
                line: one_line(&h.line, 300),
            })
            .collect();
        done(format!("{} matching lines", hits.len()), results::Hits { hits })
    }

    async fn open_port(&self, a: OpenPortArgs) -> Out {
        let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        let host = match beside {
            Some(b) => self.readable(b).await?.info.host,
            None => None,
        };
        let req = OpenRequest {
            kind: BlockType::Browser,
            config: json!({ "port": a.port, "path": a.path }),
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host,
            local: host.is_none(),
        };
        let block = self.open(req).await?;
        let place = match beside {
            Some(b) => format!("beside %{b}"),
            None => "in a new tab".into(),
        };
        done(
            format!("Opened port {} in browser block %{block} {place}", a.port),
            results::PortOpened { block, port: a.port },
        )
    }

    /// Beside `beside`, or the agent itself; on its machine.
    async fn beside(&self, beside: Option<&PaneArg>) -> Result<(Option<PaneId>, Option<u32>), String> {
        let beside = match (beside.map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        let host = match beside {
            Some(b) => self.readable(b).await?.info.host,
            None => None,
        };
        Ok((beside, host))
    }

    async fn show_changes(&self, a: ShowChangesArgs) -> Out {
        let (beside, host) = self.beside(a.beside.as_ref()).await?;
        if beside.is_none() && a.repo.is_none() {
            return Err("say whose changes: beside (a pane in the repository) or repo (a directory in it)".into());
        }
        let req = OpenRequest {
            kind: BlockType::Diff,
            config: json!({ "repo": a.repo, "rev_a": a.rev_a, "rev_b": a.rev_b }),
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host,
            local: host.is_none(),
        };
        let block = self.open(req).await?;
        // It reads in the background: wait for that, briefly.
        let deadline = Instant::now() + Duration::from_secs(30);
        let state = loop {
            let b = self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or("the block closed")?;
            let st = b.state();
            if st["loading"] != true || Instant::now() > deadline {
                break st;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        if let Some(e) = state["error"].as_str() {
            return Err(format!("diff block %{block}: {e}"));
        }
        let files: Vec<results::DiffFile> = state["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| results::DiffFile {
                path: f["path"].clone(),
                old: f["old"].clone(),
                status: f["status"].clone(),
                add: f["add"].clone(),
                del: f["del"].clone(),
                binary: f["binary"].clone(),
                big: f["big"].clone(),
            })
            .collect();
        let n = files.len();
        done(
            format!(
                "Diff block %{block}: {n} file{} changed, +{} -{} ({}); read_output on %{block} has the diff",
                if n == 1 { "" } else { "s" },
                state["add"],
                state["del"],
                state["against"].as_str().unwrap_or("")
            ),
            results::DiffOpened { block, repo: state["repo"].clone(), files },
        )
    }

    async fn show_file(&self, a: ShowFileArgs) -> Out {
        let (beside, host) = self.beside(a.beside.as_ref()).await?;
        let req = OpenRequest {
            kind: BlockType::File,
            config: json!({ "path": a.path, "line": a.line }),
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host,
            local: host.is_none(),
        };
        let block = self.open(req).await?;
        let at = a.line.map(|l| format!(" at line {l}")).unwrap_or_default();
        done(format!("Showing {}{at} in file block %{block}", a.path), results::BlockOnly { block })
    }

    /// `doc new`: the block beside the caller's pane, then its outline.
    async fn doc_new(&self, a: DocNewArgs) -> Out {
        crate::labs::flag(self.app.control.state_dir(), flags::DOCS)?;
        let (beside, host) = self.beside(a.beside.as_ref()).await?;
        let req = OpenRequest {
            kind: BlockType::Doc,
            config: json!({ "title": a.title }),
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host,
            local: host.is_none(),
        };
        // Through `open`, which records who started it: an agent block may
        // drive only what it started, so it could not fill its own Doc
        // otherwise.
        let block = self.open(req).await?;
        let sections: Vec<Value> =
            a.sections.iter().map(|s| json!({ "heading": s.heading, "intent": s.intent })).collect();
        let args = json!({ "title": a.title, "sections": sections });
        let outlined = match self.app.mux.api(|r| Api::Block(block, r)).await.flatten() {
            Some(b) => b.call_by("doc_outline", args, Some(&self.by())).await,
            None => Err("the Doc's block closed".into()),
        };
        let outline = match outlined {
            Ok(o) => o,
            Err(e) => {
                // A Doc with no outline is nothing to look at.
                let _ = self.app.mux.api(|r| Api::Close(block, r)).await;
                return Err(e);
            }
        };
        let sections = outline["sections"].clone();
        let ids: Vec<&str> = sections.as_array().into_iter().flatten().filter_map(|s| s["id"].as_str()).collect();
        done(
            format!(
                "Doc block %{block} with {} sections ({}): fill them in order with doc fill",
                ids.len(),
                ids.join(", ")
            ),
            json!({ "block": block, "sections": sections }),
        )
    }

    /// `doc fill`: one section of a Doc this caller may drive.
    async fn doc_fill(&self, a: DocFillArgs) -> Out {
        crate::labs::flag(self.app.control.state_dir(), flags::DOCS)?;
        let block = a.block.id()?;
        let p = self.drivable(block).await?;
        if p.info.kind != BlockType::Doc {
            return Err(format!("%{block} isn't a Doc: doc new makes one"));
        }
        let b =
            self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
        let args = json!({ "section": a.section, "markdown": a.markdown });
        let out = b.call_by("doc_fill", args, Some(&self.by())).await?;
        done(
            format!("Filled {} of Doc %{block} ({} bytes)", a.section, out["bytes"]),
            json!({ "block": block, "section": a.section, "bytes": out["bytes"] }),
        )
    }

    /// `doc asset`: a picture into a Doc this caller may drive, from a file
    /// of the caller's or from base64.
    async fn doc_asset(&self, a: DocAssetArgs) -> Out {
        use base64::Engine;
        crate::labs::flag(self.app.control.state_dir(), flags::DOCS)?;
        let block = a.block.id()?;
        let p = self.drivable(block).await?;
        if p.info.kind != BlockType::Doc {
            return Err(format!("%{block} isn't a Doc: doc new makes one"));
        }
        let cap = crate::docs::ASSET_MAX as u64;
        let data = match (a.path, a.data) {
            (Some(path), None) => {
                // As `attach` reads one: its paths are on its machine, not here.
                if let Some(me) = self.me()
                    && self.app.mux.api(|r| Api::MachineOf(me, r)).await.flatten().is_some()
                {
                    return Err("this agent runs on a machine of its own: send the file as data".into());
                }
                // The daemon's working directory is not the agent's.
                if !std::path::Path::new(&path).is_absolute() {
                    return Err(format!("{path}: give an absolute path, or send data"));
                }
                let size = tokio::fs::metadata(&path).await.map_err(|e| format!("{path}: {e}"))?.len();
                if size > cap {
                    return Err(format!("{path} is {size} bytes; a picture is {cap} (2 MB) at most: shrink it"));
                }
                let bytes = tokio::fs::read(&path).await.map_err(|e| format!("{path}: {e}"))?;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            }
            (None, Some(data)) => data,
            _ => return Err("give path or data".into()),
        };
        let b =
            self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or_else(|| format!("no block %{block}"))?;
        let out = b.call_by("doc_asset_put", json!({ "name": a.name, "data": data }), Some(&self.by())).await?;
        let path = out["path"].as_str().unwrap_or_default().to_owned();
        done(
            format!("Saved {path} in Doc %{block}: show it with ![what it shows]({path}) in a section"),
            json!({ "block": block, "path": path, "mime": out["mime"], "size": out["size"] }),
        )
    }
}

#[cfg(feature = "labs")]
impl Call<'_> {
    async fn open_app(&self, a: OpenAppArgs) -> Out {
        crate::labs::flag(self.app.control.state_dir(), flags::STUDIO)?;
        let studio = crate::labs::apps::studio::get().ok_or("no studio here")?;
        let Some(name) = a.app else {
            let apps = studio.apps().await?;
            let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
            return done(format!("{} apps: {}", apps.len(), names.join(", ")), results::Apps { apps });
        };
        let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        if let Some(b) = beside {
            self.readable(b).await?;
        }
        let config = crate::labs::app_config(&json!({ "app": name })).await?;
        let req = OpenRequest {
            kind: BlockType::App,
            config,
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host: None,
            local: true,
        };
        let block = self.open(req).await?;
        done(format!("Opened app {name} in block %{block}"), results::AppOpened { block, app: name })
    }

    async fn open_workspace(&self, a: OpenWorkspaceArgs) -> Out {
        let (beside, host) = self.beside(a.beside.as_ref()).await?;
        let dir = a.dir.trim_end_matches('/').to_owned();
        if !dir.starts_with('/') && !dir.starts_with("~/") {
            return Err(format!("{dir:?}: give the workspace's directory as an absolute path"));
        }
        let req = OpenRequest {
            kind: BlockType::Workspace,
            config: json!({ "root": dir, "env": a.env.as_deref().unwrap_or("local"), "actor": a.actor, "principals": a.principals }),
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host,
            local: host.is_none(),
        };
        let block = self.open(req).await?;
        // Its first read takes a second or two (four chant processes).
        let deadline = Instant::now() + Duration::from_secs(40);
        let state = loop {
            let b = self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or("the block closed")?;
            let st = b.state();
            if st["updated_ms"].as_u64().is_some_and(|t| t > 0) || Instant::now() > deadline {
                break st;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        if let Some(e) = state["error"].as_str() {
            return Err(format!("workspace block %{block}: {e}"));
        }
        let members: Vec<results::Member> = state["members"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| results::Member {
                name: m["name"].clone(),
                dir: m["dir"].clone(),
                kind: m["kind"].clone(),
                errors: m["errors"].clone(),
                gates: m["gates"].clone(),
            })
            .collect();
        let gates = state["gates"].clone();
        let waiting = gates.as_array().map_or(0, Vec::len);
        done(
            format!(
                "Workspace block %{block}: {} ({} members{}); read_output on %{block} has the whole of it",
                state["name"].as_str().unwrap_or("workspace"),
                members.len(),
                match waiting {
                    0 => String::new(),
                    1 => ", a gate waits for the user".into(),
                    n => format!(", {n} gates wait for the user"),
                }
            ),
            results::WorkspaceOpened { block, root: state["root"].clone(), members, gates },
        )
    }

    /// Delegating (M78): a task for another machine's agent.
    async fn delegate(&self, kind: &str, a: DelegateArgs) -> Out {
        use crate::labs::agents::delegate as d;
        if !crate::labs::on(self.app.control.state_dir(), flags::AGENTS) {
            return Err("delegating is in Labs: `arugulad flags agents on` turns it on here".into());
        }
        // The owner's agents': a guest's agent asks no one's agents.
        if let Some(me) = self.me()
            && self.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err("only the owner's own agents delegate to other machines".into());
        }
        let app = self.app.clone();
        // M80: the work comes back, from what's kept here.
        if kind == "review" || kind == "apply" {
            let task = a.task.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| format!("{kind} needs task"))?;
            let dir = match a.dir.as_deref().filter(|d| !d.is_empty()) {
                Some(d) => d.to_owned(),
                None => match self.me() {
                    Some(me) => self.readable(me).await?.info.cwd.ok_or("say where your checkout is: dir")?,
                    None => return Err("say where your checkout is: dir".into()),
                },
            };
            let state = app.control.state_dir().to_owned();
            if kind == "apply" {
                let r = d::apply(&state, &d::review_root(), task, std::path::Path::new(&dir)).await?;
                return done(r.text(task), json!({ "task": task, "applied": r }));
            }
            let (wt, r) = d::review(&state, &d::review_root(), task, std::path::Path::new(&dir)).await?;
            let beside = self.own_pane();
            let req = OpenRequest {
                kind: BlockType::Diff,
                config: json!({ "repo": wt }),
                split: beside,
                from_pane: beside,
                local: true,
                ..Default::default()
            };
            let block = self.open(req).await?;
            let text = format!(
                "Task {task}'s patch is in diff block %{block}, on a copy of {dir}; nothing here changed yet. {}",
                r.text(task)
            );
            return done(text, json!({ "task": task, "block": block, "worktree": wt, "applied": r }));
        }
        let machine = d::locate(&app, a.machine.as_deref(), &a.agent).await?;
        let (machine, agent) = (machine.as_str(), a.agent.as_str());
        let limit = Duration::from_secs(a.wait.unwrap_or(300).min(1800));
        let task_of = || a.task.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| format!("{kind} needs task"));
        let text_of = || a.text.as_deref().filter(|t| !t.trim().is_empty()).ok_or_else(|| format!("{kind} needs text"));
        let claims = json!({ "machine": app.hosts.name(), "pane": self.me() });
        let t = match kind {
            "send" => d::send(&app, machine, agent, text_of()?, None, claims, a.pr).await?,
            "answer" => d::send(&app, machine, agent, text_of()?, Some(task_of()?), claims, false).await?,
            "cancel" => d::cancel(&app, machine, agent, task_of()?).await?,
            _ => d::get(&app, machine, agent, task_of()?).await?,
        };
        let id = t["id"].as_str().unwrap_or_default().to_owned();
        let t = if kind != "cancel" && !limit.is_zero() && !d::settled(&t) && !id.is_empty() {
            let what = format!("{agent} on {machine}");
            match self.waiting(&what, limit, d::wait(&app, machine, agent, &id, limit)).await {
                Some(r) => r?,
                None => d::get(&app, machine, agent, &id).await?,
            }
        } else {
            t
        };
        done(d::summary(machine, agent, &t), json!({ "machine": machine, "agent": agent, "task": t }))
    }

    /// The team's agent catalog (M77): the cards each machine offers.
    async fn team_agents(&self, a: ListTeamAgentsArgs) -> Out {
        use crate::labs::agents::catalog;
        if !crate::labs::on(self.app.control.state_dir(), flags::AGENTS) {
            return Err("the agent catalog is in Labs: `arugulad flags agents on` turns it on here".into());
        }
        // The owner's agents': a guest's agent sees no catalog (#234's rule).
        if let Some(me) = self.me()
            && self.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err("only the owner's own agents read the team's agent catalog".into());
        }
        let c = catalog::get(self.app, if a.fresh { 0 } else { catalog::FRESH_MS }).await;
        let q = a.query.as_deref().map(str::to_lowercase).filter(|q| !q.trim().is_empty());
        let hit = |s: &catalog::Shelf, card: &Value| {
            q.as_ref().is_none_or(|q| {
                let hay = format!("{} {} {} {}", card["name"], card["description"], s.name, s.owner).to_lowercase();
                q.split_whitespace().all(|w| hay.contains(w))
            })
        };
        let rows: Vec<(&catalog::Shelf, &Value)> = c.agents().filter(|(s, card)| hit(s, card)).collect();
        let total = c.agents().count();
        let mut text = match rows.len() {
            0 if total == 0 => "No agents offered on any machine the user's team can reach.\n".to_owned(),
            n => format!("{n} of {total} agents on {} machines:\n", c.machines.len()),
        };
        for (s, card) in &rows {
            text.push_str(&catalog::Catalog::line(s, card));
            text.push('\n');
        }
        for s in c.machines.iter().filter(|s| s.note.is_some()) {
            text.push_str(&format!("{}: {}\n", s.name, s.note.as_deref().unwrap_or_default()));
        }
        if let Some(n) = &c.note {
            text.push_str(n);
            text.push('\n');
        }
        done(text, &c)
    }

    /// The account's agents (M43), read with the user's own login on this
    /// host.
    async fn fountain_agents(&self, profile: Option<&str>) -> Result<crate::labs::fountain::Agents, String> {
        let runner = crate::labs::fountain::local_runner(&self.app.mux.shell_env).await;
        crate::labs::fountain::agents_for(&runner, profile).await
    }

    async fn list_agents(&self, a: ListAgentsArgs) -> Out {
        let mut f = crate::labs::fountain::catalog::Filter::default();
        f.apply(&json!({ "query": a.query, "source": a.source }))?;
        let got = self.fountain_agents(a.profile.as_deref()).await?;
        let (login, agents) = (&got.login, &got.agents);
        let rows = crate::labs::fountain::rows(agents, &f);
        let mut text = match f.describe() {
            d if d.is_empty() => format!("{} agents on {}:\n", agents.len(), login.base_url),
            d => format!("{} of {} agents on {} ({d}):\n", rows.len(), agents.len(), login.base_url),
        };
        for r in &rows {
            text.push_str(&crate::labs::fountain::catalog::line(r));
            text.push('\n');
        }
        let note = crate::labs::fountain::unreadable_note(got.unreadable);
        if let Some(n) = &note {
            text.push_str(n);
            text.push('\n');
        }
        let rows: Vec<results::AgentRow> = rows
            .iter()
            .map(|c| results::AgentRow {
                name: c.name.clone(),
                id: c.id.clone(),
                runtime: c.runtime.clone(),
                model: c.model.clone(),
                source: c.source,
                app: c.app.clone(),
                skills: c.skills.clone(),
                mcp: c.mcp.clone(),
                description: c.description.clone(),
            })
            .collect();
        done(text, results::FountainAgents { total: agents.len(), agents: rows, unreadable: got.unreadable })
    }

    async fn read_agent(&self, a: ReadAgentArgs) -> Out {
        let got = self.fountain_agents(a.profile.as_deref()).await?;
        let agent = crate::labs::fountain::find(&got.agents, &a.name).ok_or_else(|| {
            format!("no agent {:?} on this Managoat account (list kind managoat_agents lists them)", a.name)
        })?;
        let recipe = crate::labs::fountain::recipe(agent);
        done(format!("{} ({}, {})", agent.name, agent.runtime, agent.model), recipe)
    }

    /// The account's conversations (#942), read with the daemon's login.
    async fn fountain_conversations(&self, a: ListFountainConversationsArgs) -> Out {
        let (text, v) =
            crate::labs::fountain::conversations_answer(&a.labels, a.status, a.limit.map(|l| l.min(500) as u32))
                .await?;
        done(text, v)
    }

    async fn open_fountain(&self, a: OpenFountainArgs) -> Out {
        let (beside, _) = self.beside(a.beside.as_ref()).await?;
        let mut filter = crate::labs::fountain::catalog::Filter::default();
        filter.apply(&json!({ "query": a.query, "source": a.source }))?;
        let config = json!({ "profile": a.profile, "filter": filter });
        let req = OpenRequest {
            kind: BlockType::Fountain,
            config,
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host: None,
            local: true,
        };
        let block = self.open(req).await?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let b = loop {
            let b = self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or("the block closed")?;
            if b.state()["loading"] != true || Instant::now() > deadline {
                break b;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let st = b.state();
        if let Some(e) = st["error"].as_str() {
            return Err(format!("Managoat block %{block}: {e}"));
        }
        done(
            format!(
                "Managoat block %{block}: {} of {} agents",
                st["agents"].as_array().map_or(0, Vec::len),
                st["total"]
            ),
            results::BlockText { block, text: b.text() },
        )
    }
}

#[cfg(not(feature = "labs"))]
impl Call<'_> {
    async fn open_app(&self, _: OpenAppArgs) -> Out {
        Err(crate::labs::not_built("An app block"))
    }

    async fn open_workspace(&self, _: OpenWorkspaceArgs) -> Out {
        Err(crate::labs::not_built("A chant workspace"))
    }

    async fn list_agents(&self, _: ListAgentsArgs) -> Out {
        Err(crate::labs::not_built("Managoat"))
    }

    async fn team_agents(&self, _: ListTeamAgentsArgs) -> Out {
        Err(crate::labs::not_built("The agent catalog"))
    }

    async fn delegate(&self, _: &str, _: DelegateArgs) -> Out {
        Err(crate::labs::not_built("Delegating"))
    }

    async fn read_agent(&self, _: ReadAgentArgs) -> Out {
        Err(crate::labs::not_built("Managoat"))
    }

    async fn open_fountain(&self, _: OpenFountainArgs) -> Out {
        Err(crate::labs::not_built("Managoat"))
    }

    async fn fountain_conversations(&self, _: ListFountainConversationsArgs) -> Out {
        Err(crate::labs::not_built("Managoat"))
    }
}

impl Call<'_> {
    /// A forge block (M36's PR, M37's issue or new issue) beside a pane.
    async fn open_forge(&self, mut what: ForgeOpen, dir: Option<String>, beside: Option<PaneArg>) -> Out {
        let (beside, host) = self.beside(beside.as_ref()).await?;
        // N alone means the repository beside it, if no dir is given.
        let dir = match (&dir, beside) {
            (Some(d), _) => Some(d.clone()),
            (None, Some(b)) if host.is_none() => self.readable(b).await?.info.cwd,
            _ => None,
        };
        what.dir = dir;
        let config = crate::forges::open_config(
            &serde_json::to_value(&what).map_err(|e| e.to_string())?,
            self.app.control.state_dir(),
        )
        .await?;
        let req = OpenRequest {
            kind: BlockType::Forge,
            config,
            session: None,
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host: None,
            local: true,
        };
        let block = self.open(req).await?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let b = loop {
            let b = self.app.mux.api(|r| Api::Block(block, r)).await.flatten().ok_or("the block closed")?;
            if b.state()["loading"] != true || Instant::now() > deadline {
                break b;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let st = forge_state(&b)?;
        let what = if st.kind == ItemKind::Issue { "Issue" } else { "PR" };
        if let Some(e) = &st.error {
            return Err(format!("{what} block %{block}: {e}"));
        }
        if st.number == 0 {
            return done(
                format!(
                    "Drafted a new issue on {} in %{block}: it waits for the user to send, edit or drop it (read_forge shows what became of it)",
                    st.repo
                ),
                results::ForgeDrafted { block, status: "waiting", text: b.text() },
            );
        }
        let title = st
            .pr
            .as_ref()
            .map(|p| p.item.title.as_str())
            .or(st.issue.as_ref().map(|i| i.item.title.as_str()))
            .unwrap_or("");
        done(
            format!("{what} block %{block}: {}#{} {title}", st.repo, st.number),
            results::ForgeOpened { block, text: b.text(), wants: st.wants },
        )
    }

    /// A forge block this caller may read.
    async fn forge(&self, block: &PaneArg) -> Result<(PaneId, Arc<dyn crate::block::Block>), String> {
        let id = block.id()?;
        let p = self.readable(id).await?;
        if p.info.kind != BlockType::Forge {
            return Err(format!("%{id} isn't a PR or issue block: show (kind pr or issue) opens one"));
        }
        let b = self.app.mux.api(|r| Api::Block(id, r)).await.flatten().ok_or_else(|| format!("no block %{id}"))?;
        Ok((id, b))
    }

    async fn read_forge(&self, a: ReadForgeArgs) -> Out {
        let (id, b) = self.forge(&a.block).await?;
        let st = forge_state(&b)?;
        let what = if st.kind == ItemKind::Issue { "Issue" } else { "PR" };
        done(
            format!("{what} block %{id}: {}#{}", st.repo, st.number),
            results::ForgeRead { block: id, text: b.text(), wants: st.wants, drafts: st.drafts, error: st.error },
        )
    }

    /// An agent's write: a draft on the block, at once.
    async fn pr_write(&self, block: &PaneArg, write: Write) -> Out {
        let (id, b) = self.forge(block).await?;
        let (method, args) = write_call(&write);
        let out = b.call_by(&method, args, Some(&self.by())).await?;
        let draft = out["draft"].as_str().unwrap_or("?").to_owned();
        done(
            format!(
                "Drafted on %{id} as {draft}: it waits for the user to send, edit or drop it (read_forge shows what became of it)"
            ),
            out,
        )
    }

    /// A new issue drafted again, on its block, after the user asked for
    /// changes.
    async fn redraft_issue(&self, block: &PaneArg, title: String, body: String) -> Out {
        let (id, b) = self.forge(block).await?;
        let out = b.call_by("redraft", json!({ "title": title, "body": body }), Some(&self.by())).await?;
        done(
            format!(
                "Drafted the new issue again in %{id}: it waits for the user to send, edit or drop it (read_forge shows what became of it)"
            ),
            out,
        )
    }

    /// An invite's panes (#234): the caller's own, and where it opens. An
    /// agent's own block, or for a full caller the pane `arugula mcp`
    /// runs in, else the one it names.
    fn invite_panes(&self, pane: Option<&PaneArg>) -> Result<(PaneId, PaneId), String> {
        let pane = pane.map(PaneArg::id).transpose()?;
        let mine = self.own_pane();
        match (mine.or(pane), pane.or(mine)) {
            (Some(from), Some(to)) => Ok((from, to)),
            _ => Err("pane: which pane's session to invite them into (arugula mcp in a pane says its own)".into()),
        }
    }

    /// Whose invite drafts these are: an agent block's, or a full client's
    /// in a pane.
    fn drafter(&self, from: PaneId) -> String {
        match self.me() {
            Some(me) => format!("%{me}"),
            None => format!("{}@%{from}", self.by()),
        }
    }

    /// The invite blocks (#234), with their sessions.
    async fn invite_blocks(&self) -> Vec<(PaneId, arugula_core::SessionId, Arc<dyn crate::block::Block>)> {
        let mut out = vec![];
        for p in self.panes().await.into_iter().filter(|p| p.info.kind == BlockType::Invite) {
            if let Some(b) = self.app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten() {
                out.push((p.info.id, p.session, b));
            }
        }
        out
    }

    async fn invite_person(&self, a: InvitePersonArgs) -> Out {
        let role = match a.role.as_deref().map(str::trim) {
            None | Some("" | "viewer") => arugula_core::Role::Viewer,
            Some("editor") => arugula_core::Role::Editor,
            Some(r) => return Err(format!("role {r}: viewer or editor (an invite never makes an owner)")),
        };
        let note = a.note.trim();
        if note.is_empty() {
            return Err("note: say why you want them there (the user reads it on the card)".into());
        }
        if note.chars().count() > crate::invite::card::NOTE_MAX {
            return Err(format!("note: at most {} characters", crate::invite::card::NOTE_MAX));
        }
        let (from, pane) = self.invite_panes(a.pane.as_ref())?;
        let at = self.readable(pane).await?;
        // The owner's agents ask; one a guest started (or an agent of
        // theirs did) learns nothing of who this machine knows.
        let started = self.started_text().await?;
        // Someone this machine knows, or no card at all.
        let person = crate::invite::resolve(self.app, &a.who, None).map_err(|(_, why)| why)?;
        if crate::invite::is_me(self.app, &person.id) {
            return Err(format!("{} owns this machine", person.name));
        }
        let drafter = self.drafter(from);
        let blocks = self.invite_blocks().await;
        let mine: Vec<_> = blocks.iter().filter(|(_, _, b)| b.config()["drafter"] == drafter.as_str()).collect();
        let waiting: u64 = mine.iter().map(|(_, _, b)| b.state()["waiting"].as_u64().unwrap_or(0)).sum();
        if waiting >= INVITES_WAITING {
            return Err(format!(
                "{waiting} of your invites wait for the user already: read_invite until they answer one"
            ));
        }
        let block = match mine.iter().find(|(_, s, _)| *s == at.session) {
            Some((id, _, b)) => (*id, b.clone()),
            None => {
                // A card of its own beside the pane, on this host; not the
                // agent's to drive or close.
                let req = OpenRequest {
                    kind: BlockType::Invite,
                    config: json!({ "drafter": drafter }),
                    session: None,
                    split: Some(pane),
                    from_pane: Some(pane),
                    vm: false,
                    image: None,
                    host: None,
                    local: true,
                };
                let id = match self.app.mux.api(|r| Api::Open(req, None, r)).await {
                    Some(r) => r?,
                    None => return Err("the daemon is shutting down".into()),
                };
                let by = StartedBy { by: self.by(), block: None };
                self.app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(id, by)));
                let b = self.app.mux.api(|r| Api::Block(id, r)).await.flatten().ok_or("the invite block closed")?;
                (id, b)
            }
        };
        let args = json!({ "who": a.who.trim(), "person": person.id, "name": person.name, "role": role,
            "note": note, "session": at.session, "session_name": at.session_name, "pane": pane, "from": from,
            "started": started });
        let out = block.1.call_by("draft", args, Some(&self.by())).await?;
        let draft = out["draft"].as_str().unwrap_or("?").to_owned();
        done(
            format!(
                "Asked the user to invite {} ({}) into {} at %{pane}: {draft} waits on their card in %{} (read_invite shows what became of it)",
                person.name,
                role.as_str(),
                at.session_name,
                block.0
            ),
            results::InviteDrafted {
                draft,
                status: "waiting",
                block: block.0,
                who: person.id,
                name: person.name,
                role,
                session: at.session,
                pane,
            },
        )
    }

    /// Who started the agent asking (#234), as its card says it: the
    /// owner, or the agent that did. An agent a guest stands behind may
    /// not ask at all.
    async fn started_text(&self) -> Result<String, String> {
        let Some(me) = self.me() else { return Ok("your own client".into()) };
        if self.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some() {
            return Err("only the owner's own agents may ask to invite someone".into());
        }
        Ok(match self.readable(me).await?.info.started_by {
            Some(StartedBy { by, block: Some(b) }) => format!("started by {by} in %{b}"),
            Some(StartedBy { by, block: None }) => format!("started by {by}"),
            None => "you started it".into(),
        })
    }

    async fn read_invite(&self, a: ReadInviteArgs) -> Out {
        let drafter = match self.me() {
            Some(me) => format!("%{me}"),
            None => self.drafter(self.invite_panes(a.pane.as_ref())?.0),
        };
        let id = a.draft.trim();
        for (block, _, b) in self.invite_blocks().await {
            if b.config()["drafter"] != drafter.as_str() {
                continue;
            }
            let st = b.state();
            let Some(d) = st["drafts"].as_array().and_then(|ds| ds.iter().find(|d| d["id"] == id)).cloned() else {
                continue;
            };
            let status = d["status"].as_str().unwrap_or("waiting").to_owned();
            let name = d["name"].as_str().unwrap_or("").to_owned();
            let by = d["settled_by"].as_str().unwrap_or("the user");
            let summary = match status.as_str() {
                "waiting" => format!("{id} waits on the user's card in %{block}"),
                "sent" => format!(
                    "{by} invited {name}; their notification: {}{}",
                    d["delivery"].as_str().unwrap_or("?"),
                    d["delivery_reason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default()
                ),
                "declined" => match d["reason"].as_str() {
                    Some(r) => format!("{by} declined inviting {name}: {r}"),
                    None => format!("{by} declined inviting {name}"),
                },
                "dropped" => format!("Nobody answered {id} in time: nothing was shared"),
                _ => format!("Inviting {name} failed: {}", d["error"].as_str().unwrap_or("?")),
            };
            return done(summary, results::InviteRead { card: d, draft: id.to_owned(), block });
        }
        // Its block was closed: what it said then.
        let root = self.app.mux.store.root().to_owned();
        let kept = tokio::task::spawn_blocking(move || crate::invite::card::closed(&root)).await.unwrap_or_default();
        if let Some(c) = kept.into_iter().rev().find(|c| c.drafter == drafter && c.draft.id == id) {
            let status = serde_json::to_value(&c.draft).unwrap_or_default()["status"].as_str().unwrap_or("").to_owned();
            let block = c.block;
            return done(
                format!("{id} is {status}; its invite block %{block} was closed"),
                results::InviteRead { card: c.draft, draft: id.to_owned(), block },
            );
        }
        Err(format!("no invite {id} of yours (invite_person returns one)"))
    }

    async fn open(&self, req: OpenRequest) -> Result<PaneId, String> {
        confine(self.on_machine().await?, open_lands_on_machine(&req))?;
        match self.app.mux.api(|r| Api::Open(req, None, r)).await {
            Some(Ok(b)) => {
                self.started(b).await;
                Ok(b)
            }
            Some(Err(e)) => Err(e),
            None => Err("the daemon is shutting down".into()),
        }
    }

    async fn start_agent(&self, a: StartAgentArgs) -> Out {
        if a.prompt.trim().is_empty() {
            return Err("give the agent a prompt".into());
        }
        let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(b), _) => Some(b),
            (None, own) => own,
        };
        if self.me().is_some() && (a.vm || a.session.is_some()) {
            return Err(
                "this agent's token reaches its own tab only: start_agent opens beside it (no vm or session)".into()
            );
        }
        let host = match beside {
            Some(b) => self.readable(b).await?.info.host,
            None => None,
        };
        let recipe = a.recipe.as_deref().map(str::trim).filter(|n| !n.is_empty());
        may_start(self.on_machine().await?, host.is_some(), a.vm, recipe.is_some())?;
        // #942 F4: the recipe on Managoat, as its Managoat agent.
        match a.on.as_deref().map(str::trim) {
            None | Some("" | "here") => {}
            // `fountain`: Managoat's name before the rename, still read.
            Some("managoat" | "fountain") => return self.start_on_fountain(&a, recipe, beside).await,
            Some(o) => return Err(format!("on: here or managoat, not {o}")),
        }
        if recipe.is_some() && !matches!(a.agent, AgentKind::Claude) {
            return Err("recipe is for agent claude: Claude Code runs the recipe".into());
        }
        // An agent doesn't hand another one every check switched off.
        if let Some(m) = a.permission_mode.as_deref().filter(|m| SKIPS_CHECKS.contains(m)) {
            return Err(format!("permission_mode {m} skips every check: only the user starts an agent like that"));
        }
        if host.is_some() {
            self.share_my_machine().await;
        }
        let agent = match a.agent {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Fountain => "managoat",
            AgentKind::Acp => "acp",
        };
        let mut config = json!({ "agent": agent, "prompt": a.prompt });
        if let Some(c) = &a.command {
            config["command"] = json!(c);
        }
        if let Some(f) = &a.fountain_agent {
            config["managoat_agent"] = json!(f);
        }
        if let Some(r) = recipe {
            config["recipe"] = json!(r);
        }
        if let Some(m) = &a.model {
            config["model"] = json!(m);
        }
        if let Some(t) = a.title.as_ref().filter(|t| !t.trim().is_empty()) {
            config["title"] = json!(t.trim());
        }
        if let Some(c) = &a.cwd {
            config["cwd"] = json!(c);
        }
        // #163: a lead pre-authorizing what it hands out.
        if !a.allow.is_empty() {
            config["allow"] = a.allow.iter().map(|t| json!({ "tool": t })).collect();
        }
        if let Some(m) = &a.permission_mode {
            config["permission_mode"] = json!(m);
        }
        if a.user_settings {
            config["user_settings"] = json!(true);
        }
        // #379: Claude Code's login is the caller's (`arugula mcp` says
        // its client's directory), not the daemon's default one. Without
        // it, a block beside an agent or a pane takes theirs (the mux).
        if matches!(a.agent, AgentKind::Claude | AgentKind::Acp)
            && !a.vm
            && host.is_none()
            && let Some(d) = self.claude_config_dir()
        {
            config["claude_config_dir"] = json!(d);
        }
        let req = OpenRequest {
            kind: BlockType::Agent,
            config,
            session: if beside.is_none() { a.session.clone() } else { None },
            split: beside,
            from_pane: beside,
            vm: a.vm,
            image: None,
            host: if a.vm { None } else { host },
            local: false,
        };
        let block = self.open(req).await?;
        // #858: a lead's helper is one of its links.
        if let Some(conv) = self.caller.scope.leads() {
            let title = a.title.clone().filter(|t| !t.trim().is_empty());
            let name = crate::labs::lead::adopt(self.app, conv, block, title).await.map_err(|e| e.1)?;
            return done(
                format!(
                    "Started an agent in %{block}, linked as {name}: wait on {name} (until needs_input or idle), then read_output for what it said"
                ),
                results::BlockOnly { block },
            );
        }
        done(
            format!(
                "Started an agent in %{block}; wait on it (until needs_input or idle), then read_output for its transcript"
            ),
            results::BlockOnly { block },
        )
    }

    /// `start_agent {recipe, on: managoat}`: the recipe upserted as its
    /// Managoat agent, and its conversation in an agent block.
    async fn start_on_fountain(&self, a: &StartAgentArgs, recipe: Option<&str>, beside: Option<PaneId>) -> Out {
        let Some(recipe) = recipe else {
            return Err("on: managoat runs a recipe: give recipe (a Claude Code subagent's name)".into());
        };
        if a.vm {
            return Err("on: managoat runs in a Managoat sandbox, not a VM".into());
        }
        if !matches!(a.agent, AgentKind::Claude | AgentKind::Fountain) {
            return Err("on: managoat runs the recipe as Claude Code".into());
        }
        let ask = crate::labs::FountainRecipe {
            agent: recipe.to_owned(),
            cwd: a.cwd.clone(),
            prompt: Some(a.prompt.clone()),
            vault: a.vault.clone(),
            environment: a.environment.clone(),
        };
        let (mut config, summary) = crate::labs::fountain_recipe(self.app, ask).await?;
        if let Some(t) = a.title.as_ref().filter(|t| !t.trim().is_empty()) {
            config["title"] = json!(t.trim());
        }
        let req = OpenRequest {
            kind: BlockType::Agent,
            config,
            session: if beside.is_none() { a.session.clone() } else { None },
            split: beside,
            from_pane: beside,
            vm: false,
            image: None,
            host: None,
            local: false,
        };
        let block = self.open(req).await?;
        done(
            format!(
                "{summary} Started in %{block}; wait on it (until needs_input or idle), then read_output for its transcript"
            ),
            results::BlockOnly { block },
        )
    }

    async fn prompt_agent(&self, a: PromptArgs) -> Out {
        use arugula_proto::{
            api::{PromptRequest, PromptResult},
            op::ops::PanePrompt,
        };
        let pane = a.pane.id()?;
        self.drivable(pane).await?;
        let limit = Self::limit(a.timeout);
        let cx = Cx { app: self.app, via: Via::Mcp(self) };
        let req =
            PromptRequest { text: a.text, answering: a.answering, stall: None, timeout: Some(limit.as_secs_f64()) };
        let fut = PanePrompt::handle(&cx, pane, req);
        let r = self.waiting(&format!("waiting for %{pane}'s turn"), limit, fut).await;
        // The handler has its own deadline, the same `limit`: whichever
        // fires first, the answer at the limit is the same.
        let r = match r {
            Some(r) => r.map_err(|e| e.http().1)?,
            None => return self.not_yet(pane, limit, Some("end of its turn")).await,
        };
        if r == PromptResult::StillRunning {
            return self.not_yet(pane, limit, Some("end of its turn")).await;
        }
        let summary = match &r {
            PromptResult::Done => {
                format!("%{pane} finished its turn; read_output (screen: true for what it shows) for what it said")
            }
            PromptResult::NeedsInput { question, .. } => {
                format!("%{pane} asks: {}; agent_respond answers it", question.as_deref().unwrap_or("for someone"))
            }
            PromptResult::Blocked { question, .. } => format!(
                "%{pane} was already waiting on someone ({}), so nothing was typed; agent_respond answers it, or pass answering",
                question.as_deref().unwrap_or("a question")
            ),
            PromptResult::Stalled { why, .. } => format!("%{pane} stalled: {why}"),
            PromptResult::StillRunning => format!("%{pane} is still working: wait until idle"),
        };
        done(summary, results::PromptedAgent { result: r, pane })
    }

    async fn respond(&self, a: RespondArgs) -> Out {
        use arugula_proto::Action;
        let pane = a.pane.id()?;
        self.drivable(pane).await?;
        let (action, content) = match a.action {
            Response::Allow => (Action::Allow, None),
            Response::Deny | Response::Skip => (Action::Deny, None),
            Response::Answer => {
                let answers = a.answers.clone().ok_or("answer needs answers: the question's fields, by name")?;
                (Action::Answer, Some(Value::Object(answers)))
            }
        };
        if let Some(o) = a.option.as_deref()
            && !matches!(o, "once" | "always")
        {
            return Err(format!("option {o}: once or always"));
        }
        let req = ActRequest {
            action,
            pane: Some(pane),
            panes: vec![],
            id: a.id.clone(),
            content,
            option: a.option.clone(),
            suggestion: None,
            message: a.message.clone(),
            text: None,
        };
        crate::api::act_as(self.app, pane, &req, self.driver()).await?;
        let did = match a.action {
            Response::Allow => "Allowed",
            Response::Deny => "Denied",
            Response::Answer => "Answered",
            Response::Skip => "Skipped",
        };
        done(format!("{did} what %{pane} asked"), results::PaneOnly { pane })
    }

    async fn read_file(&self, a: ReadFileArgs) -> Out {
        let max = a.max_chars.unwrap_or(PAGE).clamp(200, PAGE_MAX);
        let pane = match (a.pane.as_ref().map(PaneArg::id).transpose()?, self.own_pane()) {
            (Some(p), _) => Some(p),
            (None, own) => own,
        };
        let mut path = a.path.clone();
        if let Some(p) = pane {
            let info = self.readable(p).await?;
            if !path.starts_with('/')
                && !path.starts_with('~')
                && let Some(cwd) = info.info.cwd.as_deref()
            {
                path = format!("{}/{path}", cwd.trim_end_matches('/'));
            }
        }
        let offset = a.offset.unwrap_or(0);
        let (bytes, size) = crate::fs::read_on(self.app, pane, &path, offset, max as u64).await?;
        if bytes.iter().take(8000).any(|b| *b == 0) {
            return Err(format!("{path} looks binary ({size} bytes); read_file reads text"));
        }
        // Whole characters only: a page may end mid-character.
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        let mut used = bytes.len();
        if text.ends_with('\u{fffd}') && (offset + bytes.len() as u64) < size {
            let cut = text.char_indices().last().map(|(i, _)| i).unwrap_or(0);
            text.truncate(cut);
            used = text.len().min(used);
        }
        let next = offset + used as u64;
        let more = next < size;
        let summary = if more {
            format!("{path}: bytes {offset}–{next} of {size}; more: call again with offset {next}")
        } else {
            format!("{path}: {size} bytes, to the end")
        };
        done(summary, results::FileRead { path, size, offset, next_offset: next, more, text })
    }

    // ------------------------------------------------------------ resources

    /// A resource's text: `arugula://history`, `arugula://pane/N/output`,
    /// `arugula://pane/N/screen` or `arugula://block/N`.
    pub async fn resource(&self, uri: &str) -> Result<String, String> {
        let rest = uri.strip_prefix("arugula://").ok_or_else(|| format!("not an Arugula resource: {uri}"))?;
        let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        let pane_of = |s: &str| PaneArg::Name(s.to_owned()).id();
        let out = match parts.as_slice() {
            ["history"] => {
                self.history(HistoryArgs {
                    failed: false,
                    kind: None,
                    pattern: None,
                    since: None,
                    before: None,
                    cwd: None,
                    matching: None,
                    pane: None,
                    limit: Some(50),
                })
                .await?
            }
            ["pane", id, "output"] => {
                let v = self
                    .read_output(ReadArgs {
                        pane: PaneArg::Id(pane_of(id)?),
                        offset: None,
                        last_command: false,
                        max_chars: None,
                        screen: false,
                    })
                    .await?;
                return Ok(v["text"].as_str().unwrap_or_default().to_owned());
            }
            ["pane", id, "screen"] => {
                let v = self.capture(PaneOnly { pane: PaneArg::Id(pane_of(id)?) }).await?;
                return Ok(v["text"].as_str().unwrap_or_default().to_owned());
            }
            ["block", id] => {
                let id = pane_of(id)?;
                let p = self.readable(id).await?;
                let state = match self.app.mux.api(|r| Api::Block(id, r)).await.flatten() {
                    Some(b) => b.state(),
                    None => Value::Null,
                };
                serde_json::to_value(results::BlockResource { pane: entry(&p), state }).unwrap_or_default()
            }
            _ => return Err(format!("no such resource: {uri}")),
        };
        Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
    }
}

/// A pane in `list`.
pub(crate) fn entry(p: &PaneSummary) -> results::PaneEntry {
    let i = &p.info;
    results::PaneEntry {
        pane: i.id,
        kind: i.kind,
        session: p.session_name.clone(),
        tab: p.tab,
        tab_name: p.tab_name.clone(),
        machine: i.host.map(|m| format!("m{m}")),
        cwd: i.cwd.clone(),
        command: i.command.clone(),
        title: i.title.clone(),
        running: i.running,
        attention: i.attention,
        why: i.reason.as_ref().map(|r| r.headline.clone()),
        current: i.current.as_ref().and_then(|c| c.text.clone()),
        last: i.last.as_ref().map(|c| results::LastCommand { command: c.text.clone(), exit: c.exit }),
        started_by: i.started_by.as_ref().map(|s| s.by.clone()),
        you: None,
    }
}

/// A write as the forge block's `call_by` takes it: the method, and its
/// arguments (the write without its `method` tag).
fn write_call(write: &Write) -> (String, Value) {
    let mut args = serde_json::to_value(write).unwrap_or_default();
    let method = args.as_object_mut().and_then(|o| o.remove("method")).and_then(|m| m.as_str().map(str::to_owned));
    (method.unwrap_or_default(), args)
}

/// A forge block's state, as the block's view has it.
fn forge_state(b: &Arc<dyn crate::block::Block>) -> Result<ForgeState, String> {
    serde_json::from_value(b.state()).map_err(|e| format!("the forge block's state: {e}"))
}

/// A block's text (an agent's transcript), paged by character.
fn page_text(pane: PaneId, text: &str, offset: Option<u64>, max: usize) -> Out {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let from = match offset {
        Some(o) => (o as usize).min(total),
        None => total.saturating_sub(max),
    };
    let to = (from + max).min(total);
    let page: String = chars[from..to].iter().collect();
    let more = to < total;
    let summary = if more {
        format!("%{pane}: characters {from}–{to} of {total}; more: call again with offset {to}")
    } else {
        format!("%{pane}: characters {from}–{to}, to the end")
    };
    done(summary, results::Page { pane, offset: from, next_offset: to, more, text: page })
}

/// The longest start of `raw` whose text fits in `max` characters, cut at a
/// line's end where one is near: (text, raw bytes used).
fn head_page(raw: &[u8], max: usize) -> (String, usize) {
    let all = strip(raw);
    if all.len() <= max {
        return (all, raw.len());
    }
    let (mut lo, mut hi) = (0usize, raw.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if strip(&raw[..mid]).len() <= max { lo = mid } else { hi = mid - 1 }
    }
    let cut = match raw[..lo].iter().rposition(|b| *b == b'\n') {
        Some(nl) if nl + 1 > lo / 2 => nl + 1,
        _ => lo,
    };
    (strip(&raw[..cut]), cut)
}

/// The shortest end of `raw` whose text fits in `max` characters, from a
/// line's start where one is near: (text, raw bytes skipped).
fn tail_page(raw: &[u8], max: usize) -> (String, usize) {
    let all = strip(raw);
    if all.len() <= max {
        return (all, 0);
    }
    let (mut lo, mut hi) = (0usize, raw.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if strip(&raw[mid..]).len() <= max { hi = mid } else { lo = mid + 1 }
    }
    let skip = match raw[lo..].iter().position(|b| *b == b'\n') {
        Some(nl) if nl < (raw.len() - lo) / 2 => lo + nl + 1,
        _ => lo,
    };
    (strip(&raw[skip..]), skip)
}

/// The end of `s`, at most `max` bytes, on a character boundary.
fn cap_tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut cut = s.len() - max;
    while !s.is_char_boundary(cut) {
        cut += 1;
    }
    s[cut..].to_owned()
}

fn one_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max).collect();
    out.push('…');
    out
}

/// `90m`, `36h`, `2d`, `45s` or plain seconds.
fn seconds(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = n.parse().map_err(|_| format!("{s:?}: a duration like 90m, 36h, 2d or seconds"))?;
    let mult = match unit.trim() {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        u => return Err(format!("{s:?}: unit {u} (use s, m, h or d)")),
    };
    Ok(n * mult)
}

fn ago(ms: u64) -> String {
    let secs = now_ms().saturating_sub(ms) / 1000;
    match secs {
        0..60 => format!("{secs}s ago"),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

/// The one rule for every tool that creates something (a shell, an agent,
/// a block): an agent on a machine (a guest's, say) creates things on a
/// machine, never on this host, where they'd run as the owner (with the
/// owner's logins and, for a worn recipe, secrets).
pub(crate) fn confine(caller_on_machine: bool, lands_on_machine: bool) -> Result<(), String> {
    if caller_on_machine && !lands_on_machine {
        return Err("an agent on a machine creates things on its machine (or a new VM), not on this host".into());
    }
    Ok(())
}

/// Where `run` puts its shell: a machine when it joins one or makes one.
fn run_lands_on_machine(req: &RunRequest) -> bool {
    req.join || req.vm || req.vm_tab || req.sandbox.is_some()
}

/// Where a block opens: a machine when it has one or makes one.
fn open_lands_on_machine(req: &OpenRequest) -> bool {
    req.vm || req.host.is_some()
}

/// Whether `start_agent` may start this agent ([`confine`]), and a worn
/// recipe on this host only.
fn may_start(caller_on_machine: bool, on_machine: bool, vm: bool, worn: bool) -> Result<(), String> {
    confine(caller_on_machine, on_machine || vm)?;
    if worn && (on_machine || vm) {
        return Err("a worn agent (a recipe) runs on this host, not on a machine".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Every Labs flag on, or none.
    fn fl(labs: bool) -> arugula_proto::flags::On {
        if labs { arugula_proto::flags::On::all() } else { arugula_proto::flags::On::none() }
    }

    #[test]
    fn who_may_start_what_where() {
        use super::may_start;
        // The owner's agents (and the CLI), here.
        assert!(may_start(false, false, false, true).is_ok());
        assert!(may_start(false, false, false, false).is_ok());
        assert!(may_start(false, true, false, false).is_ok());
        // A guest's VM agent: on its machine or a VM, never here.
        assert!(may_start(true, true, false, false).is_ok());
        assert!(may_start(true, false, true, false).is_ok());
        assert!(may_start(true, false, false, false).unwrap_err().contains("not on this host"));
        assert!(may_start(true, false, false, true).unwrap_err().contains("not on this host"));
        // A worn agent never goes on a machine.
        assert!(may_start(false, true, false, true).is_err());
        assert!(may_start(false, false, true, true).is_err());
    }

    /// run, open (show's blocks, start_agent's…) and show's conversation
    /// all go through `confine`.
    #[test]
    fn an_agent_on_a_machine_creates_on_machines() {
        use super::*;
        // run: a split of the owner's local pane is a shell here.
        let here = RunRequest { split: Some(3), join: false, ..Default::default() };
        let there = RunRequest { split: Some(4), join: true, ..Default::default() };
        let vm = RunRequest { vm: true, ..Default::default() };
        assert!(confine(true, run_lands_on_machine(&here)).unwrap_err().contains("not on this host"));
        assert!(confine(true, run_lands_on_machine(&there)).is_ok());
        assert!(confine(true, run_lands_on_machine(&vm)).is_ok());
        assert!(confine(false, run_lands_on_machine(&here)).is_ok(), "the owner's agents, as before");
        // open: a block without a machine is this host's.
        let block = |host: Option<u32>, vm: bool| OpenRequest {
            kind: BlockType::Agent,
            config: json!({}),
            session: None,
            split: Some(3),
            from_pane: Some(3),
            vm,
            image: None,
            host,
            local: host.is_none(),
        };
        assert!(confine(true, open_lands_on_machine(&block(None, false))).is_err());
        assert!(confine(true, open_lands_on_machine(&block(Some(2), false))).is_ok());
        assert!(confine(true, open_lands_on_machine(&block(None, true))).is_ok());
        assert!(confine(false, open_lands_on_machine(&block(None, false))).is_ok());
        // show kind conversation: always this host's.
        assert!(confine(true, false).is_err() && confine(false, false).is_ok());
    }

    use super::*;

    /// `tools/list` for every scope, with and without labs, against the
    /// checked-in file: names, descriptions, annotations and every input
    /// schema. `ARUGULA_BLESS=1` rewrites the file after a change meant to
    /// alter what agents see. The file is a Labs build's (which has the
    /// forge too): a build without Labs leaves out Managoat's kinds and
    /// arguments (#942, F5) and has no golden of its own, so it checks the
    /// listing in `unlisted_tools_are_not_listed_without_labs_but_are_there`.
    #[test]
    fn the_tool_list_and_each_schema_are_unchanged() {
        if !crate::labs::BUILT {
            return;
        }
        let mut all = serde_json::Map::new();
        for (scope_name, scope) in [("full", Scope::Full), ("read", Scope::Read), ("block", Scope::Block(1))] {
            for labs in [true, false] {
                let tools = serde_json::to_value(list(scope, fl(labs))).unwrap();
                all.insert(format!("{scope_name}, labs {labs}"), tools);
            }
        }
        let got = serde_json::to_string_pretty(&Value::Object(all)).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp-tool-list.json");
        if std::env::var_os("ARUGULA_BLESS").is_some() {
            std::fs::write(&path, &got).unwrap();
        }
        let want =
            std::fs::read_to_string(&path).expect("tests/fixtures/mcp-tool-list.json (ARUGULA_BLESS=1 writes it)");
        assert!(got == want, "the tool list changed; ARUGULA_BLESS=1 rewrites {}", path.display());
    }

    /// #580: an `outputSchema` is an object at the root (MCP's rule) that
    /// says what `done` adds, `summary`, and no grouped tool lists one.
    #[test]
    fn an_output_schema_is_an_object_with_a_summary() {
        let mut with = 0;
        for tool in list(Scope::Full, fl(true)) {
            let grouped = all_defs().into_iter().any(|d| d.name == tool.name && matches!(d.args, Args::Kinds { .. }));
            let Some(schema) = &tool.output_schema else {
                continue;
            };
            with += 1;
            assert!(!grouped, "{} answers by kind and lists an outputSchema", tool.name);
            let schema = Value::Object((**schema).clone());
            assert_eq!(schema["type"], "object", "{}", tool.name);
            assert_eq!(schema["properties"]["summary"], json!({ "type": "string" }), "{}", tool.name);
            assert!(schema["required"].as_array().unwrap().contains(&json!("summary")), "{}", tool.name);
            let mut found = vec![];
            fn formats(v: &Value, found: &mut Vec<String>) {
                match v {
                    Value::Object(o) => {
                        if let Some(f) = o.get("format").and_then(Value::as_str) {
                            found.push(f.to_owned());
                        }
                        o.values().for_each(|x| formats(x, found));
                    }
                    Value::Array(a) => a.iter().for_each(|x| formats(x, found)),
                    _ => {}
                }
            }
            formats(&schema, &mut found);
            assert!(found.iter().all(|f| STANDARD_FORMATS.contains(&f.as_str())), "{}: {found:?}", tool.name);
        }
        assert!(with >= 8, "only {with} tools list an outputSchema");
    }

    /// A build without Labs lists nothing of Managoat, studio apps or chant
    /// workspaces, whatever the `labs` flag says: no kind, no argument, no
    /// value, no word in a description.
    #[cfg(not(feature = "labs"))]
    #[test]
    fn a_build_without_labs_never_lists_fountain() {
        let dir = std::env::temp_dir().join(format!("arugula-nolabs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in arugula_proto::flags::FLAGS {
            arugula_proto::flags::set(&dir, f.name, true).unwrap();
        }
        assert_eq!(arugula_proto::flags::on(&dir), arugula_proto::flags::On::all(), "the flags are on");
        let labs = crate::labs::flags(&dir).any();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!labs, "labs is on in a build without it");
        for scope in [Scope::Full, Scope::Read, Scope::Block(1)] {
            let tools = serde_json::to_string(&list(scope, fl(labs))).unwrap().to_lowercase();
            assert!(!tools.contains("fountain") && !tools.contains("managoat"), "the tool list mentions Managoat");
            assert!(!tools.contains("studio"), "the tool list mentions studio");
            assert!(!tools.contains("workspace"), "the tool list mentions workspaces");
            assert!(!tools.contains("read_thread"), "the tool list mentions chat");
            assert!(!tools.contains("post_thread"), "the tool list mentions chat");
        }
        let shown = kinds_of(labs, "show");
        assert!(!shown.contains(&"app") && !shown.contains(&"workspace"), "{shown:?}");
    }

    /// What `pr_write` hands the forge block's `call_by`: the keys the old
    /// `json!` wrote, but a missing body or style is left out, not `null`
    /// (the block reads both as none).
    #[test]
    fn a_write_is_a_method_and_its_arguments() {
        let call = |w: Write| write_call(&w);
        assert_eq!(call(Write::Comment { body: "hi".into() }), ("comment".into(), json!({ "body": "hi" })));
        assert_eq!(
            call(Write::Review { event: ReviewEvent::RequestChanges, body: Some("no".into()) }),
            ("review".into(), json!({ "event": "request_changes", "body": "no" }))
        );
        assert_eq!(
            call(Write::Review { event: ReviewEvent::Approve, body: None }),
            ("review".into(), json!({ "event": "approve" }))
        );
        assert_eq!(call(Write::Merge { style: None }), ("merge".into(), json!({})));
        assert_eq!(call(Write::Merge { style: Some("squash".into()) }), ("merge".into(), json!({ "style": "squash" })));
    }

    #[test]
    fn pages_cut_at_lines_and_fit() {
        let raw: Vec<u8> = (0..2000).flat_map(|i| format!("\x1b[32mline {i:04}\x1b[0m\r\n").into_bytes()).collect();
        let (text, used) = head_page(&raw, 1000);
        assert!(text.len() <= 1000 && text.len() > 900, "{}", text.len());
        assert!(text.starts_with("line 0000\n") && text.ends_with('\n'), "{text:?}");
        let (next, _) = head_page(&raw[used..], 1000);
        assert!(next.starts_with("line "), "the next page starts on a line: {next:?}");
        let (tail, skipped) = tail_page(&raw, 1000);
        assert!(tail.len() <= 1000 && tail.ends_with("line 1999\n"), "{tail:?}");
        assert!(tail.starts_with("line "), "{tail:?}");
        assert_eq!(strip(&raw[skipped..]), tail);
        let (small, used) = head_page(b"hi\r\n", 1000);
        assert_eq!((small.as_str(), used), ("hi\n", 4));
    }

    #[test]
    fn durations_and_panes() {
        assert_eq!(seconds("2d"), Ok(172800));
        assert_eq!(seconds("90m"), Ok(5400));
        assert_eq!(seconds("45"), Ok(45));
        assert!(seconds("2w").is_err());
        assert_eq!(PaneArg::Name("%7".into()).id(), Ok(7));
        assert_eq!(PaneArg::Id(3).id(), Ok(3));
        assert!(PaneArg::Name("x".into()).id().is_err());
    }

    #[test]
    fn annotations_are_honest() {
        let all = list(Scope::Full, fl(false));
        assert_eq!(all.len(), 19 - NO_FORGE);
        let ro: Vec<&str> = all
            .iter()
            .filter(|t| t.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true))
            .map(|t| t.name.as_ref())
            .collect();
        let mut want =
            vec!["read_output", "wait", "list", "capabilities", "history", "read_forge", "read_invite", "read_file"];
        want.retain(|n| crate::forges::BUILT || !FORGE_TOOLS.contains(n));
        assert_eq!(ro, want);
        assert_eq!(list(Scope::Read, fl(false)).len(), ro.len(), "a read token sees the read-only tools only");
        let hint = |n: &str| all.iter().find(|t| t.name == n).unwrap().annotations.clone().unwrap();
        assert_eq!(hint("close").destructive_hint, Some(true));
        // A group is as careful as its most careful kind: draft has merge.
        if crate::forges::BUILT {
            assert_eq!(hint("draft").destructive_hint, Some(true));
        }
        assert_eq!((hint("show").destructive_hint, hint("show").open_world_hint), (Some(false), Some(true)));
    }

    /// Chat's two tools, unlisted without the `chat` flag (`Op::FLAG`).
    const CHAT_TOOLS: [&str; 2] = ["read_thread", "post_thread"];

    /// How many of the tools a build without the forge doesn't list.
    const NO_FORGE: usize = if crate::forges::BUILT { 0 } else { FORGE_TOOLS.len() };

    /// The kinds that aren't listed without a flag: the table's, and the
    /// operations' own (`Op::FLAG`).
    fn flagged_kinds() -> Vec<(&'static str, &'static str)> {
        let mut all: Vec<_> = UNLISTED.iter().map(|(t, k, _)| (*t, *k)).collect();
        for d in all_defs() {
            if let Args::Kinds { kinds, .. } = &d.args {
                all.extend(kinds.iter().filter(|k| k.flag.is_some()).map(|k| (d.name, k.name)));
            }
        }
        all
    }

    /// The kinds of a group a tool has, listed with `labs` or without.
    fn kinds_of(labs: bool, tool: &str) -> Vec<&'static str> {
        defs(fl(labs))
            .into_iter()
            .find(|d| d.name == tool)
            .map(|d| match d.args {
                Args::Kinds { kinds, .. } => kinds.iter().map(|k| k.name).collect(),
                Args::One(_) => Vec::new(),
            })
            .unwrap_or_default()
    }

    /// Managoat, studio apps, chant workspaces and chat are for what a
    /// stranger doesn't have: without `labs` chat's tools and the others'
    /// kinds aren't listed (18 tools of 20), with it all are, and a caller
    /// who names one reaches it either way.
    #[test]
    fn unlisted_tools_are_not_listed_without_labs_but_are_there() {
        for scope in [Scope::Full, Scope::Read] {
            let listed: Vec<String> = list(scope, fl(false)).iter().map(|t| t.name.to_string()).collect();
            for name in CHAT_TOOLS {
                assert!(!listed.contains(&name.to_string()), "{name} is listed for {scope:?} without labs");
            }
            let with: Vec<String> = list(scope, fl(true)).iter().map(|t| t.name.to_string()).collect();
            for d in all_defs().iter().filter(|d| scope == Scope::Full || d.read_only) {
                if !crate::forges::BUILT && FORGE_TOOLS.contains(&d.name) {
                    continue;
                }
                assert!(with.contains(&d.name.to_string()), "{} isn't listed for {scope:?} with labs", d.name);
            }
        }
        // A hidden kind is filtered out of its group: not in kind's enum,
        // nor its line in the description, nor the arguments only it takes.
        for (tool, kind) in flagged_kinds() {
            assert!(!kinds_of(false, tool).contains(&kind), "{tool} kind {kind} is listed without labs");
            assert!(kinds_of(true, tool).contains(&kind), "{tool} kind {kind} isn't listed with labs");
            // Still there: by its kind, and by its name before #349.
            route(tool, json!({ "kind": kind }), fl(false)).unwrap_or_else(|e| panic!("{tool} {kind}: {e}"));
            // A tool all of whose kinds are flagged (`doc`) isn't listed at all.
            let Some(t) = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == tool) else {
                assert!(defs(fl(false)).iter().all(|d| d.name != tool), "{tool}");
                continue;
            };
            let kinds = t.input_schema["properties"]["kind"]["enum"].clone();
            assert!(!kinds.as_array().unwrap().contains(&json!(kind)), "{tool}: {kinds}");
            assert!(!t.description.as_deref().unwrap().contains(&format!("\n- {kind}")), "{tool}: {kind}");
        }
        for old in ["open_app", "open_workspace"] {
            let (def, kind, _) = route(old, json!({}), fl(false)).unwrap_or_else(|e| panic!("{old}: {e}"));
            assert!(flagged_kinds().contains(&(def.name, kind.unwrap())), "{old}");
        }
        let show = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == "show").unwrap();
        for arg in ["app", "env"] {
            assert!(show.input_schema["properties"].get(arg).is_none(), "show takes {arg} without labs");
        }
        // Retired kinds are never advertised. Old calls still route to
        // the compatibility handlers, which explain the migration.
        let list_tool = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == "list").unwrap();
        for (tool, kind) in FOUNTAIN_KINDS {
            assert!(!kinds_of(false, tool).contains(&kind), "{tool} kind {kind}");
            route(tool, json!({ "kind": kind }), fl(false)).unwrap_or_else(|e| panic!("{tool} {kind}: {e}"));
        }
        for old in ["open_fountain", "list_agents", "read_agent"] {
            let (def, kind, _) = route(old, json!({}), fl(false)).unwrap_or_else(|e| panic!("{old}: {e}"));
            assert!(FOUNTAIN_KINDS.contains(&(def.name, kind.unwrap())), "{old}");
        }
        // Their names before the rename (DECISIONS.md "Fountain is
        // Managoat") still reach them, and aren't listed.
        for (tool, old, now) in [
            ("show", "fountain", "managoat"),
            ("list", "fountain_agents", "managoat_agents"),
            ("list", "fountain_agent", "managoat_agent"),
            ("list", "fountain_conversations", "managoat_conversations"),
        ] {
            let (def, kind, _) =
                route(tool, json!({ "kind": old }), fl(false)).unwrap_or_else(|e| panic!("{old}: {e}"));
            assert_eq!((def.name, kind), (tool, Some(now)));
            assert!(!kinds_of(false, tool).contains(&old), "{tool} lists {old}");
        }
        let a: StartAgentArgs =
            parse(json!({ "agent": "fountain", "fountain_agent": "arena", "prompt": "hi", "on": "fountain" })).unwrap();
        assert!(matches!(a.agent, AgentKind::Fountain) && a.fountain_agent.as_deref() == Some("arena"));
        for (t, arg) in
            [(&show, "query"), (&show, "source"), (&show, "profile"), (&list_tool, "name"), (&list_tool, "source")]
        {
            assert!(t.input_schema["properties"].get(arg).is_none(), "{} {arg}", t.name);
        }
        let start = list(Scope::Full, fl(false)).into_iter().find(|t| t.name == "start_agent").unwrap();
        for arg in FOUNTAIN_ARGS[0].1 {
            assert!(start.input_schema["properties"].get(*arg).is_none(), "start_agent {arg}");
        }
        // An error names only the kinds that are listed.
        let e = route("show", json!({ "kind": "terminal" }), fl(false)).err().unwrap();
        assert!(e.contains("port") && !e.contains("managoat") && !e.contains("workspace"), "{e}");
        let every: Vec<&str> = all_defs().iter().map(|d| d.name).collect();
        for name in CHAT_TOOLS {
            assert!(every.contains(&name), "{name} no longer has a definition");
        }
        assert_eq!(every.len(), 23);
        assert_eq!(defs(fl(false)).len(), 19 - NO_FORGE);
        assert_eq!(defs(fl(true)).len(), 23 - NO_FORGE);
        assert_eq!(every.len(), defs(fl(false)).len() + CHAT_TOOLS.len() + 2 + NO_FORGE, "and delegate and doc");
    }

    /// The 39 tools before #349, by their old names.
    const BEFORE: [&str; 39] = [
        "run",
        "send_input",
        "attach",
        "read_output",
        "capture_screen",
        "wait",
        "list",
        "close",
        "read_thread",
        "post_thread",
        "history",
        "search",
        "open_port",
        "open_app",
        "start_agent",
        "list_conversations",
        "open_conversation",
        "prompt_agent",
        "agent_respond",
        "show_changes",
        "show_file",
        "open_workspace",
        "open_pr",
        "read_pr",
        "pr_comment",
        "pr_review",
        "pr_merge",
        "open_issue",
        "read_issue",
        "issue_comment",
        "issue_new",
        "invite_person",
        "read_invite",
        "list_agents",
        "read_agent",
        "open_fountain",
        "read_file",
        "list_devices",
        "device_call",
    ];

    /// Tools added after #349 for jobs no old tool did: delegating (M78),
    /// saying what's here (#229), and building a Doc (#963).
    const SINCE_TOOLS: usize = 3;

    /// Kinds added after #349, each a job no old tool did.
    const SINCE: &[(&str, Option<&str>)] = &[
        ("list", Some("agents")),
        ("list", Some("managoat_conversations")),
        ("capabilities", None),
        ("delegate", Some("send")),
        ("delegate", Some("get")),
        ("delegate", Some("answer")),
        ("delegate", Some("cancel")),
        ("delegate", Some("review")),
        ("delegate", Some("apply")),
        // A conversation's lead's links (#858).
        ("list", Some("links")),
        // Building a Doc (#963).
        ("doc", Some("new")),
        ("doc", Some("fill")),
        // Its pictures (#965).
        ("doc", Some("asset")),
    ];

    /// About half as many tools (#349), and every old job is still there:
    /// each old name is a tool now, or reaches the tool and kind that does
    /// its job, with its old arguments; and it stays read-only or not.
    #[test]
    fn every_old_tool_is_a_tool_or_a_kind() {
        let now = all_defs();
        // The old jobs in half as many tools; a tool for a job none of them
        // did (SINCE) counts apart.
        let new_tools =
            now.iter().filter(|d| SINCE.iter().any(|(t, _)| *t == d.name) && !BEFORE.contains(&d.name)).count();
        let new_tools = new_tools.min(SINCE_TOOLS);
        assert!((now.len() - new_tools) * 2 <= BEFORE.len() + 1, "{} tools", now.len());
        let mut jobs = std::collections::BTreeSet::new();
        for old in BEFORE {
            let (def, kind, _) = route(old, json!({}), fl(true)).unwrap_or_else(|e| panic!("{old}: {e}"));
            let was_ro = matches!(
                old,
                "read_output"
                    | "capture_screen"
                    | "wait"
                    | "list"
                    | "read_thread"
                    | "history"
                    | "search"
                    | "list_conversations"
                    | "read_pr"
                    | "read_issue"
                    | "read_invite"
                    | "list_agents"
                    | "read_agent"
                    | "read_file"
                    | "list_devices"
            );
            assert_eq!(def.read_only, was_ro, "{old} is now {}, read-only {}", def.name, def.read_only);
            // search is history's kind output, an argument of a tool that
            // does one thing.
            jobs.insert((def.name, kind));
        }
        // Jobs since: the team's agent catalog (M77).
        jobs.extend(SINCE.iter().copied());
        // Every tool and kind there is now does one of the old jobs, or a new one.
        let mut all = std::collections::BTreeSet::new();
        for d in &now {
            match &d.args {
                Args::One(_) => {
                    all.insert((d.name, None));
                }
                Args::Kinds { kinds, .. } => all.extend(kinds.iter().map(|k| (d.name, Some(k.name)))),
            }
        }
        assert_eq!(jobs, all);
    }

    /// The old tools' arguments still parse as the kind that does their
    /// job, kind added.
    #[test]
    fn old_calls_reach_their_kind_with_their_arguments() {
        let calls = [
            ("capture_screen", json!({ "pane": 3 }), "read_output", None),
            ("search", json!({ "pattern": "x", "since": "1d", "limit": 5 }), "history", None),
            ("list_conversations", json!({ "query": "q", "live": true }), "list", Some("conversations")),
            ("list_devices", json!({}), "list", Some("devices")),
            ("list_agents", json!({ "query": "q", "source": "hand" }), "list", Some("managoat_agents")),
            ("read_agent", json!({ "name": "pr-reviewer" }), "list", Some("managoat_agent")),
            ("open_port", json!({ "port": 3000, "path": "/x", "beside": "%2" }), "show", Some("port")),
            ("show_changes", json!({ "repo": "/r", "rev_a": "main" }), "show", Some("changes")),
            ("show_file", json!({ "path": "a.rs", "line": 4 }), "show", Some("file")),
            ("open_pr", json!({ "pr": "o/r#1", "dir": "/r" }), "show", Some("pr")),
            ("open_issue", json!({ "issue": "o/r#2" }), "show", Some("issue")),
            ("open_conversation", json!({ "id": "abc", "then": "fork" }), "show", Some("conversation")),
            ("open_app", json!({ "app": "hud" }), "show", Some("app")),
            ("open_workspace", json!({ "dir": "/w", "env": "prod" }), "show", Some("workspace")),
            ("open_fountain", json!({ "query": "q", "source": "hand" }), "show", Some("managoat")),
            ("read_pr", json!({ "block": 4 }), "read_forge", None),
            ("read_issue", json!({ "block": "%4" }), "read_forge", None),
            ("pr_comment", json!({ "block": 4, "body": "b" }), "draft", Some("comment")),
            ("issue_comment", json!({ "block": 4, "body": "b" }), "draft", Some("comment")),
            ("pr_review", json!({ "block": 4, "event": "request_changes", "body": "b" }), "draft", Some("review")),
            ("pr_merge", json!({ "block": 4, "style": "squash" }), "draft", Some("merge")),
            ("issue_new", json!({ "repo": "o/r", "title": "t" }), "draft", Some("issue")),
        ];
        for (old, args, tool, kind) in calls {
            let (def, k, args) = route(old, args, fl(false)).unwrap_or_else(|e| panic!("{old}: {e}"));
            assert_eq!((def.name, k), (tool, kind), "{old}");
            let ok = match (tool, kind) {
                ("read_output", _) => parse::<ReadArgs>(args).map(|a| assert!(a.screen)).is_ok(),
                ("history", _) => parse::<HistoryArgs>(args)
                    .map(|a| assert_eq!((a.kind.as_deref(), a.pattern.as_deref()), (Some("output"), Some("x"))))
                    .is_ok(),
                ("list", Some("conversations")) => parse::<ListConversationsArgs>(args).is_ok(),
                ("list", Some("devices")) => parse::<NoArgs>(args).is_ok(),
                ("list", Some("managoat_agents")) => parse::<ListAgentsArgs>(args).is_ok(),
                ("list", Some("managoat_agent")) => parse::<ReadAgentArgs>(args).is_ok(),
                ("show", Some("port")) => parse::<OpenPortArgs>(args).is_ok(),
                ("show", Some("changes")) => parse::<ShowChangesArgs>(args).is_ok(),
                ("show", Some("file")) => parse::<ShowFileArgs>(args).is_ok(),
                ("show", Some("pr")) => parse::<OpenPrArgs>(args).is_ok(),
                ("show", Some("issue")) => parse::<OpenIssueArgs>(args).is_ok(),
                ("show", Some("conversation")) => parse::<OpenConversationArgs>(args).is_ok(),
                ("show", Some("app")) => parse::<OpenAppArgs>(args).is_ok(),
                ("show", Some("workspace")) => parse::<OpenWorkspaceArgs>(args).is_ok(),
                ("show", Some("managoat")) => parse::<OpenFountainArgs>(args).is_ok(),
                ("read_forge", _) => parse::<ReadForgeArgs>(args).is_ok(),
                ("draft", Some("comment")) => parse::<CommentArgs>(args).is_ok(),
                ("draft", Some("review")) => parse::<PrReviewArgs>(args).is_ok(),
                ("draft", Some("merge")) => parse::<PrMergeArgs>(args).is_ok(),
                ("draft", Some("issue")) => parse::<IssueNewArgs>(args).is_ok(),
                _ => unreachable!("{tool} {kind:?}"),
            };
            assert!(ok, "{old}'s arguments don't parse as {tool} {kind:?}");
        }
    }

    /// A grouped call says what's wrong with it: no kind, a kind it doesn't
    /// have, or an argument its kind doesn't take. list has a default kind;
    /// show and draft don't.
    #[test]
    fn kinds_are_picked_and_checked() {
        let kind = |tool: &str, args: Value| route(tool, args, fl(true)).map(|(_, k, a)| (k, a));
        assert_eq!(kind("list", json!({})).unwrap(), (Some("panes"), json!({})));
        assert_eq!(kind("list", json!({ "kind": "conversations", "live": true })).unwrap().0, Some("conversations"));
        let e = kind("show", json!({ "port": 3000 })).unwrap_err();
        assert!(e.contains("show needs a kind") && e.contains("port, changes, file"), "{e}");
        let e = kind("show", json!({ "kind": "terminal" })).unwrap_err();
        assert!(e.contains("no kind \"terminal\""), "{e}");
        let e = kind("show", json!({ "kind": 3 })).unwrap_err();
        assert!(e.contains("kind is a string"), "{e}");
        let e = kind("show", json!({ "kind": "port", "port": 3000, "line": 4 })).unwrap_err();
        assert!(e.contains("line isn't an argument of show kind port: it takes port, beside?, path?"), "{e}");
        let e = kind("list", json!({ "kind": "devices", "query": "x" })).unwrap_err();
        assert!(e.contains("it takes nothing else"), "{e}");
        let (k, a) = kind("draft", json!({ "kind": "merge", "block": 4 })).unwrap();
        assert_eq!((k, a), (Some("merge"), json!({ "block": 4 })), "kind is taken out");
        // history isn't a group: its kind is its own argument, passed on.
        let (k, a) = kind("history", json!({ "kind": "answer" })).unwrap();
        assert_eq!((k, a), (None, json!({ "kind": "answer" })));
        assert!(kind("nope", json!({})).unwrap_err().contains("no tool nope"));
    }

    /// A grouped tool's schema has every kind's arguments, kind as an enum
    /// of its kinds, and a description that says what each kind takes. An
    /// argument several kinds share is the same thing in each, give or take
    /// being optional.
    #[test]
    fn grouped_schemas_merge_their_kinds() {
        for labs in [false, true] {
            for d in defs(fl(labs)) {
                let Args::Kinds { kinds, default } = d.args else { continue };
                let s = grouped_schema(&kinds, default);
                let props = s["properties"].as_object().unwrap();
                let names: Vec<&str> = kinds.iter().map(|k| k.name).collect();
                assert_eq!(props["kind"]["enum"], json!(names), "{}", d.name);
                assert_eq!(
                    s.get("required").is_some(),
                    default.is_none(),
                    "{}: kind is required without a default",
                    d.name
                );
                let text = kinds_text(d.description, &kinds, default);
                for k in &kinds {
                    let ks = (k.schema)();
                    assert!(text.contains(&format!("\n- {}", k.name)), "{}: {text}", d.name);
                    for (name, p) in ks.get("properties").and_then(Value::as_object).into_iter().flatten() {
                        let merged = &props[name];
                        let strip = |v: &Value| {
                            let mut v = v.clone();
                            if let Some(o) = v.as_object_mut() {
                                o.remove("description");
                            }
                            v
                        };
                        if strip(merged) != strip(p) {
                            assert!(
                                nullable(merged) && !nullable(p),
                                "{} {name}: {merged} vs {p} ({})",
                                d.name,
                                k.name
                            );
                        }
                    }
                    // The refs it uses are there.
                    for r in ks.get("$defs").and_then(Value::as_object).into_iter().flatten().map(|(n, _)| n) {
                        assert!(s["$defs"].get(r).is_some(), "{} lacks $defs/{r}", d.name);
                    }
                }
            }
        }
        // What show's port and file mean by path, both said.
        let show = grouped_schema(&table(SHOW), None);
        let path = show["properties"]["path"]["description"].as_str().unwrap();
        assert!(path.starts_with("port: The page's path") && path.contains("file: The file."), "{path}");
        let text = kinds_text("", &table(SHOW), None);
        assert!(text.contains("\n- port {port, beside?, path?}: "), "{text}");
        assert!(text.contains("\n- file {path, beside?, line?}: "), "{text}");
        let text = kinds_text("", &table(LIST), Some("panes"));
        assert!(text.contains("\n- panes (the default): "), "{text}");
    }

    /// What a stranger can't use isn't in the instructions or in the tools'
    /// descriptions and arguments either; chat's text is there only with
    /// `labs`.
    #[test]
    fn the_prose_leaves_out_what_a_stranger_cant_use() {
        let mut prose = vec![crate::mcp::instructions(fl(false))];
        prose.extend(list(Scope::Full, fl(false)).iter().map(|t| {
            format!(
                "{}: {} {}",
                t.name,
                t.description.as_deref().unwrap_or_default(),
                Value::Object((*t.input_schema).clone())
            )
        }));
        for text in &prose {
            let mut lower = text.to_lowercase();
            // Managoat left Labs (#942): a build with it lists it to anyone,
            // its sandboxes too. A build without it says neither.
            if crate::labs::BUILT {
                lower = lower.replace("managoat sandbox", "").replace("sandbox provider", "").replace("managoat", "");
            }
            for word in [
                "fountain",
                "managoat",
                "studio",
                "chant",
                "workspace",
                "throwaway vm",
                "sandbox",
                "wisp",
                "open_app",
                "thread",
                "@agent",
                "\"vm\"",
                "vm_tab",
                "\"image\"",
                "\"machine\"",
                " vm",
                "(s33)",
                "(m43)",
            ] {
                assert!(!lower.contains(word), "{word} in: {text}");
            }
        }
        // With labs, run and start_agent take them again.
        let with = list(Scope::Full, fl(true));
        for (tool, args, _) in UNLISTED_ARGS {
            let t = with.iter().find(|t| t.name == tool).unwrap();
            for arg in args {
                assert!(t.input_schema["properties"].get(*arg).is_some(), "{tool} lacks {arg} with labs");
            }
        }
        let start = with.iter().find(|t| t.name == "start_agent").unwrap();
        let managoat = Value::Object((*start.input_schema).clone()).to_string().contains("\"managoat\"");
        assert!(!managoat, "retired direct execution is never advertised");
        let with = crate::mcp::instructions(fl(true));
        assert!(with.contains("read_thread and post_thread") && with.contains("@agent"), "{with}");
        // Labs only adds the thread and Doc sentences.
        let without = crate::mcp::instructions(fl(false));
        let plain = with.replace(crate::mcp::THREAD_INSTRUCTIONS, "").replace(crate::mcp::DOC_INSTRUCTIONS, "");
        assert!(with.len() > without.len() && plain == without);
        assert!(with.contains("doc to build a report") && !without.contains("doc to build"), "{with}");
    }

    /// #963: `doc` is a tool, with `new` and `fill`, only where `docs` is on;
    /// then `capabilities` offers it. Without the flag it is not listed, and
    /// its schema and prose are nowhere.
    #[test]
    fn doc_is_listed_only_with_its_flag() {
        use arugula_proto::flags::On;
        let docs = |on: On| defs(on).into_iter().find(|d| d.name == "doc");
        assert!(docs(On::none()).is_none());
        assert!(docs(On::all()).is_some());
        let Some(Def { args: Args::Kinds { kinds, .. }, .. }) = docs(On::all()) else { panic!("doc has kinds") };
        assert_eq!(kinds.iter().map(|k| k.name).collect::<Vec<_>>(), ["new", "fill"]);
        let off = block_offers(On::none());
        assert!(off.iter().all(|o| o.kind != "doc"), "{off:?}");
        let on = block_offers(On::all());
        let offer = on.iter().find(|o| o.kind == "doc").expect("capabilities offers a doc");
        assert_eq!(offer.open, "doc {kind: \"new\"}");
        // The schema carries the outline's shape.
        let tool = list(Scope::Full, On::all()).into_iter().find(|t| t.name == "doc").unwrap();
        let schema = Value::Object((*tool.input_schema).clone()).to_string();
        assert!(schema.contains("heading") && schema.contains("intent") && schema.contains("markdown"), "{schema}");
    }

    /// docs/mcp-permissions.json allows the read-only tools and asks for the
    /// rest, every one of them: a new tool fails this until it's listed there
    /// (#114). docs.arugula.io shows the same snippet, and the site repo's
    /// check-code holds it to this file.
    #[test]
    fn permissions_snippet_is_every_tool() {
        const SNIPPET: &str = include_str!("../../../../docs/mcp-permissions.json");
        let list = |key: &str| -> Vec<&str> {
            let at = SNIPPET
                .find(&format!("\"{key}\": ["))
                .unwrap_or_else(|| panic!("no {key} list in docs/mcp-permissions.json"));
            let body = &SNIPPET[at..];
            let body = &body[..body.find(']').unwrap()];
            body.split('"')
                .filter_map(|s| s.strip_prefix("mcp__arugula__"))
                .filter(|n| crate::forges::BUILT || !FORGE_TOOLS.contains(n))
                .collect()
        };
        // The snippet lists what a stranger sees; the two chat tools join with labs.
        let defs = defs(fl(false));
        let want = |ro: bool| -> Vec<&str> { defs.iter().filter(|d| d.read_only == ro).map(|d| d.name).collect() };
        assert_eq!(
            list("allow"),
            want(true),
            "docs/mcp-permissions.json's allow list: the read-only tools, in defs() order"
        );
        assert_eq!(list("ask"), want(false), "docs/mcp-permissions.json's ask list: every other tool, in defs() order");
    }
}
