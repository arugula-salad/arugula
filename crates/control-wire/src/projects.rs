//! Who's reading the platform page, and their projects (M150, "The project is
//! the unit of platform sharing"): their personal project and each team's
//! default one, with the team's members as the platform's projection of the
//! roster keeps them. Read only here: rosters change in the shell, signed by
//! a device ("The platform page is on app.arugula.io, not on the keys'
//! origin").
//!
//! New with M150, so nothing older reads them; the web client's copy is
//! generated (`just proto-ts`, the `ts` feature).

use serde::{Deserialize, Serialize};

/// `GET /api/projects`: who's signed in, and their projects, theirs first.
pub const PROJECTS: &str = "/api/projects";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformProjects"))]
pub struct Projects {
    pub me: Me,
    pub projects: Vec<Project>,
    /// Hosted workspaces are open to this person (M179's allowlist): only
    /// then does the shell offer to delegate a project to control (M178).
    /// A platform from before leaves it out.
    #[serde(default)]
    pub hosted: bool,
}

/// The signed-in person, as the hub knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformMe"))]
pub struct Me {
    /// Control's id for the account.
    pub account: String,
    /// Its sign-in login (GitHub's), or "".
    pub login: String,
    /// The name it chose, else its login, else "".
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
// The daemon's `Project` (a pane's) is another type, so the web client's
// copy is `PlatformProject`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PlatformProject"))]
pub struct Project {
    /// `prj_` and 32 hex characters.
    pub id: String,
    pub name: String,
    /// The team it's the default project of; `None` for the person's own.
    pub team: Option<ProjectTeam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProjectTeam {
    pub id: String,
    pub name: String,
    /// The reader's role in it: `owner`, `editor` or `viewer`.
    pub role: String,
    pub members: Vec<ProjectMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProjectMember {
    pub account: String,
    /// What the roster calls them.
    pub name: String,
    pub role: String,
}

/// `GET /api/projects/{project}/repos` (M163): the GitHub repos linked to a
/// project, for its members. `POST` links one ([`LinkRepo`]) and `DELETE
/// /api/projects/{project}/repos/{owner}/{name}` unlinks it. `{project}` is
/// a project's id, or `personal` for the caller's own.
pub const PROJECT_REPOS: &str = "/api/projects/{project}/repos";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkedRepos {
    /// The project's id (`personal` asked is answered with the real one).
    pub project: String,
    pub repos: Vec<LinkedRepo>,
}

/// A repo linked to a project, or unlinked and waiting for the purge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkedRepo {
    /// GitHub's id for it.
    pub repo_id: u64,
    /// `OWNER/NAME`.
    pub full_name: String,
    pub private: bool,
    /// Who linked it (an account id), when that account is still there.
    pub linked_by: Option<String>,
    pub linked_at: u64,
    /// When it was unlinked: nothing is read from GitHub for it since.
    pub unlinked_at: Option<u64>,
    /// When its rows go, unless it's linked again first.
    pub purge_after: Option<u64>,
    /// The project mirrors its PRs' and issues' bodies and comments (M166).
    #[serde(default)]
    pub mirror_content: bool,
}

/// `PATCH /api/projects/{project}/repos/{owner}/{name}` (M166): a linked
/// repo's settings; the answer is its [`LinkedRepo`]. `mirror_content`
/// on keeps its PRs' and issues' bodies and comments on the platform
/// (search finds their text); off deletes them at once. Turning it on
/// takes what linking takes: a member who maintains the repo on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoSettings {
    pub mirror_content: bool,
}

/// `POST /api/projects/{project}/repos`: link `OWNER/NAME`. A project
/// member who is an admin or maintainer of it on GitHub may. When the
/// GitHub App isn't installed where it lives, the answer is a 409 with
/// `reason: "install_app"` and `install_url`, GitHub's page to install it,
/// which brings them back to the project's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkRepo {
    pub repo: String,
}

/// `GET /api/projects/{project}/items` (M165): a project's PRs and issues
/// from its linked repos, last updated first, each checked against the
/// reader's own GitHub access. A query narrows it: `repo` (`OWNER/NAME`),
/// `kind` (`pr`, `issue`), `state` (`open`, `closed`, `merged`), `author`
/// and `review_requested` (GitHub logins), `q` (text in the title, or in
/// the body, comments and reviews of a repo that mirrors content) and
/// `limit` (at most 200, 100 by default).
pub const PROJECT_ITEMS: &str = "/api/projects/{project}/items";

/// `GET /api/forge/items/{id}`: one item, [`ForgeItem`]; `/checks` its head
/// commit's checks ([`ForgeChecks`]) and `/events` what happened to it
/// ([`ForgeEvents`]). An item the reader may not see is no item at all to
/// them (404).
pub const FORGE_ITEM: &str = "/api/forge/items/{id}";
pub const FORGE_ITEM_CHECKS: &str = "/api/forge/items/{id}/checks";
pub const FORGE_ITEM_EVENTS: &str = "/api/forge/items/{id}/events";
/// `GET /api/forge/items/{id}/content` (M166): its body, comments, reviews'
/// text and review comments ([`ForgeContent`]), through the same check:
/// from GitHub (kept a few minutes in memory), or from the mirror for a
/// repo whose project mirrors content.
pub const FORGE_ITEM_CONTENT: &str = "/api/forge/items/{id}/content";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeItems {
    /// The project's id (`personal` asked is answered with the real one).
    pub project: String,
    pub items: Vec<ForgeItem>,
    /// The project's linked repos whose items the reader may not see, and
    /// why: their names are the project's, which its members see anyway.
    pub withheld: Vec<WithheldRepo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WithheldRepo {
    /// `OWNER/NAME`.
    pub repo: String,
    /// `no_github` (the reader didn't sign in to control with GitHub),
    /// `no_access` (GitHub doesn't give them access), `busy` (too many
    /// lookups lately: it shows again within the hour) or `no_app` (this
    /// control has no GitHub App to ask with).
    pub why: String,
}

/// A PR or an issue, as metadata: never its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeItem {
    /// The platform's id: `fi_` and 32 hex characters.
    pub id: String,
    /// `OWNER/NAME`.
    pub repo: String,
    pub number: u64,
    /// `pr` or `issue`.
    pub kind: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    pub title: String,
    pub labels: Vec<String>,
    /// The author's GitHub login.
    pub author: Option<String>,
    pub assignees: Vec<String>,
    /// Requested reviewers' logins.
    pub reviewers: Vec<String>,
    pub draft: bool,
    pub merge_state: Option<String>,
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    pub base_ref: Option<String>,
    /// GitHub's times, in milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeChecks {
    pub item: String,
    /// The head commit they're on; `None` for an issue.
    pub head_sha: Option<String>,
    pub checks: Vec<ForgeCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeCheck {
    /// `check_run` or `status`.
    pub kind: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeEvents {
    pub item: String,
    pub events: Vec<ForgeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeEvent {
    pub id: String,
    pub kind: String,
    /// Who did it: a GitHub login.
    pub actor: Option<String>,
    /// A review's state (`approved`, `changes_requested`, `commented`).
    pub review_state: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeContent {
    pub item: String,
    /// `github` (read now, or a few minutes ago) or `mirror`.
    pub from: String,
    pub body: Option<ForgeText>,
    /// Comments, reviews' text and review comments, oldest first. A review
    /// comment's thread is the ones whose `reply_to` is its `id`.
    pub comments: Vec<ForgeText>,
}

/// A body, a comment, a review's text or a review comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeText {
    /// `body`, `comment`, `review` or `review_comment`.
    pub kind: String,
    /// GitHub's id: the PR's or issue's for a body.
    pub id: u64,
    /// A GitHub login.
    pub author: Option<String>,
    /// Markdown, as GitHub has it.
    pub body: String,
    /// A review comment's file and line.
    pub path: Option<String>,
    pub line: Option<u64>,
    /// The review comment it answers.
    pub reply_to: Option<u64>,
    /// GitHub's times, in milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}

/// `GET /api/forge/repos/{owner}/{name}` (M170): whether a GitHub repo is
/// linked to a project the reader may read it in, so a machine keeps its
/// blocks' drafts for it on the platform. Always a 200: `linked: false`
/// for a repo nobody linked, one linked to a project the reader isn't in,
/// and one they may not read (then `why`, as [`WithheldRepo`]'s, when it's
/// their project's).
pub const FORGE_REPO: &str = "/api/forge/repos/{owner}/{name}";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeRepo {
    /// `OWNER/NAME`, as asked.
    pub repo: String,
    pub linked: bool,
    /// The project it's linked to, when it's linked for the reader.
    #[serde(default)]
    pub project: Option<String>,
    /// GitHub's id for it, likewise.
    #[serde(default)]
    pub repo_id: Option<u64>,
    #[serde(default)]
    pub why: Option<String>,
    /// M172: the reader's GitHub login, when it's linked for them: who
    /// "you" are to a block pinned to one of its items, which reads no
    /// GitHub of its own.
    #[serde(default)]
    pub login: Option<String>,
}

/// Drafts (M170): a comment, a review or a new issue on a linked repo,
/// written by a person or by an agent on their machine, waiting for a
/// person to send it (M168). A draft is read by the people who may read
/// its repo's items (M165) in the repo's project; its author or any of
/// them may discard it.
///
/// - `POST /api/forge/drafts` ([`NewDraft`]) makes one, answered
///   [`DraftCreated`]. `GET` lists the reader's own, across projects.
/// - `GET /api/forge/drafts/{id}`: one ([`ForgeDraft`]); `POST
///   /api/forge/drafts/{id}/discard` discards it.
/// - `GET /api/projects/{project}/drafts`: a project's, and `GET
///   /api/forge/items/{id}/drafts` an item's.
///
/// Every list ([`ForgeDrafts`]) is the waiting ones, oldest first, unless
/// the query says `state` (`waiting`, `sent`, `discarded`, `failed` or
/// `all`); a project's takes `repo` (`OWNER/NAME`), `number` and `mine`
/// too.
pub const FORGE_DRAFTS: &str = "/api/forge/drafts";
pub const FORGE_DRAFT: &str = "/api/forge/drafts/{id}";
pub const FORGE_DRAFT_DISCARD: &str = "/api/forge/drafts/{id}/discard";
pub const PROJECT_DRAFTS: &str = "/api/projects/{project}/drafts";
pub const FORGE_ITEM_DRAFTS: &str = "/api/forge/items/{id}/drafts";

/// The longest body a draft takes: GitHub's own limit for a comment.
pub const MAX_DRAFT_BODY: usize = 65_536;

/// A draft to make. From a person (their own), or from a machine (an
/// agent's on it, for its owner: the platform records the machine, and
/// `agent` names the agent).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewDraft {
    /// `OWNER/NAME`: a repo linked to a project the author is in.
    pub repo: String,
    /// The PR or issue; `None` for a new issue.
    #[serde(default)]
    pub number: Option<u64>,
    /// `comment`, `review` or `issue`.
    pub kind: String,
    /// A review's: `COMMENT`, `APPROVE` or `REQUEST_CHANGES`.
    #[serde(default)]
    pub event: Option<String>,
    /// A new issue's.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: String,
    /// The agent that wrote it (`mcp:claude-code`), from a machine.
    #[serde(default)]
    pub agent: Option<String>,
    /// A machine's own id for a draft it kept and now uploads: the same
    /// key makes no second draft, and is answered with the first. Only a
    /// machine gives one.
    #[serde(default)]
    pub import_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DraftCreated {
    pub draft: ForgeDraft,
    /// False when an `import_key` named one made before.
    pub created: bool,
}

/// What `POST /api/forge/drafts/{id}/discard` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DraftDiscarded {
    pub id: String,
    /// `discarded`.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeDrafts {
    pub drafts: Vec<ForgeDraft>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeDraft {
    /// The platform's id: `dft_` and 32 hex characters.
    pub id: String,
    pub project: String,
    /// `OWNER/NAME`.
    pub repo: String,
    pub repo_id: u64,
    pub number: Option<u64>,
    /// The mirrored item it's on, and its title, when the mirror has it.
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub item_title: Option<String>,
    /// `comment`, `review` or `issue`.
    pub kind: String,
    pub event: Option<String>,
    pub title: Option<String>,
    pub body: String,
    /// `waiting`, `sending` (a send holds it, M168), `sent`, `discarded`
    /// or `failed`.
    pub state: String,
    /// The person it's for: an account id.
    pub author: String,
    /// The agent that wrote it, and the machine it was on (an id, and its
    /// name when the machine is still there).
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub machine_name: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub sent_at: Option<u64>,
    #[serde(default)]
    pub sent_url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// The inbox (M171): the PRs and issues that want something of the
/// reader, worked out and pushed by the platform from the mirror's rows,
/// for the same reasons a forge block asks for attention. Each entry is an
/// item and a reason ([`INBOX_REASONS`]); an item is in it while the
/// reason holds, and only for those the viewer check lets read its repo
/// (M165). Mentions and assignments on native issues, and mentions in an
/// item's thread, are entries too (M202), until they're called done; a
/// native issue's only for its project's members.
///
/// - `GET /api/inbox` ([`Inbox`]): the operation `inbox.list`. Newest
///   first, without the entries called done unless the query says
///   `done=true`; `limit` up to 500.
/// - `POST /api/inbox/{item}` ([`InboxMark`]): mark its entries (one
///   reason's, or all) seen or done, or not; answered [`InboxMarked`].
/// - `POST /api/inbox/shadow` ([`InboxShadow`]): an opted-in machine's
///   forge blocks' attention, which the platform compares with its own
///   inbox for the machine's owner and logs (the shadow period). Answered
///   [`ShadowCompared`].
pub const INBOX: &str = "/api/inbox";
pub const INBOX_ITEM: &str = "/api/inbox/{item}";
pub const INBOX_SHADOW: &str = "/api/inbox/shadow";

/// Why an item is in an inbox: a review is requested from you, your PR's
/// checks failed, changes are requested on your PR, you're mentioned, an
/// issue is assigned to you, your PR merged or is approved and green (or
/// your issue closed), a draft on it waits for you to send, or you're
/// mentioned in its thread on the platform (M202: a mirrored item's; on a
/// native issue that's `mentioned`), or a run failed (M215).
pub const INBOX_REASONS: [&str; 9] = [
    "review_requested",
    "checks_failed",
    "changes_requested",
    "mentioned",
    "assigned",
    "done",
    "draft",
    "thread_mention",
    "run_failed",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Inbox {
    pub entries: Vec<InboxEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InboxEntry {
    /// The item: `fi_` and 32 hex characters, a native issue's `iss_` id
    /// (M202), or a run's `run_` id (M215).
    pub item: String,
    /// One of [`INBOX_REASONS`].
    pub reason: String,
    /// `OWNER/NAME`; a native issue's or a run's project's name.
    pub repo: String,
    /// 0 for a run.
    pub number: u64,
    /// `pr`, `issue` or `run`.
    pub kind: String,
    /// `open`, `closed` or `merged`; a run's state.
    pub state: String,
    pub title: String,
    /// A GitHub login; a native issue's author's name.
    #[serde(default)]
    pub author: Option<String>,
    /// The item on GitHub; a native issue's or a run's path on the
    /// platform page (`/issues/<id>`, `/runs/<id>`).
    pub url: String,
    /// A native issue's or a run's project (M202, M215).
    #[serde(default)]
    pub project: Option<String>,
    /// A `thread_mention`'s room: the thread on the item (M202).
    #[serde(default)]
    pub room: Option<String>,
    /// When it came into the inbox, and when the item last changed.
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub seen_at: Option<u64>,
    #[serde(default)]
    pub done_at: Option<u64>,
}

/// Mark an item's entries: `true` sets seen or done (now, if it wasn't),
/// `false` clears it, and one not given stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InboxMark {
    /// Only this reason's entry; all of the item's when `None`.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub seen: Option<bool>,
    #[serde(default)]
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InboxMarked {
    pub item: String,
    /// How many entries it changed.
    pub marked: u32,
}

/// What an opted-in machine's forge blocks want of its owner (the shadow
/// period, M171): items and reasons, never a title.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InboxShadow {
    pub blocks: Vec<ShadowBlock>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShadowBlock {
    /// `OWNER/NAME` on github.com.
    pub repo: String,
    pub number: u64,
    /// What the block's attention rules find now, as [`INBOX_REASONS`]
    /// (`draft` aside), whether or not the person looked.
    pub reasons: Vec<String>,
    /// The platform's waiting drafts the block shows (`dft_` ids): its
    /// `draft` reason, when one is the owner's.
    #[serde(default)]
    pub drafts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShadowCompared {
    /// Blocks on items the platform has, and how many agreed.
    pub compared: u32,
    pub agree: u32,
    pub disagree: u32,
    /// Differences the platform can't help by design (a mention where
    /// content isn't mirrored): not in `compared`.
    #[serde(default)]
    pub explained: u32,
}

/// `POST /api/forge/drafts/{id}/send` ([`SendDraft`], M168): a person sends
/// a waiting (or failed) draft of their own, as themselves on GitHub: a
/// person or a CLI device, never a machine ("Agents draft, people send").
/// `via: "browser"` answers with `authorize`, GitHub's page to open in the
/// person's browser, which comes back to control and sends with a one-use
/// user token; `via: "machine"` (the default) sends it now through one of
/// their online machines, with `gh`. With neither, it's refused with
/// [`NO_WAY`] and the draft waits, saying so.
pub const FORGE_DRAFT_SEND: &str = "/api/forge/drafts/{id}/send";

/// `POST /api/forge/drafts/{id}/machine` ([`MachineSending`]): a machine
/// sends one of its owner's drafts with `gh` because a person answered its
/// block's card (M168), and says so: `start` holds the draft while it
/// sends, then `sent` or `failed`. Answered with the draft. Nothing is sent
/// by the platform here.
pub const FORGE_DRAFT_MACHINE: &str = "/api/forge/drafts/{id}/machine";

/// What a send that has no way to go says (M168).
pub const NO_WAY: &str = "Open this on a device to send, or wake a machine.";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SendDraft {
    /// `browser` or `machine` (the default).
    #[serde(default)]
    pub via: Option<String>,
    /// For `browser`: the platform page's path to come back to.
    #[serde(default)]
    pub back: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DraftSent {
    pub draft: ForgeDraft,
    /// `via: "browser"`: GitHub's page to open; the draft isn't sent yet.
    #[serde(default)]
    pub authorize: Option<String>,
    /// How it went: `token` (as the person, through GitHub's authorization)
    /// or `machine`.
    #[serde(default)]
    pub via: Option<String>,
    /// The machine's name, when one sent it.
    #[serde(default)]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineSending {
    /// `start`, `sent` or `failed`.
    pub step: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// `GET /api/projects/{project}/trackers`: the Plane projects connected to
/// a project, and those waiting for the purge, for its members. `POST`
/// connects one ([`ConnectTracker`]); `PATCH` and `DELETE` on
/// [`PROJECT_TRACKER`] change its settings and disconnect it.
pub const PROJECT_TRACKERS: &str = "/api/projects/{project}/trackers";

/// `PATCH /api/projects/{project}/trackers/{connection}` ([`TrackerSettings`])
/// and `DELETE` (disconnect).
pub const PROJECT_TRACKER: &str = "/api/projects/{project}/trackers/{connection}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerConnections {
    /// The project's id (`personal` asked is answered with the real one).
    pub project: String,
    pub connections: Vec<TrackerConnection>,
}

/// A tracker project connected to a project, or disconnected and waiting
/// for the purge. Never the key or the webhook secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerConnection {
    /// `tc_` and hex.
    pub id: String,
    /// `plane`.
    pub provider: String,
    pub workspace_slug: String,
    /// The tracker's own id for the project (Plane's uuid).
    pub source_project_id: String,
    /// The project's short key, such as `ARUGU`.
    pub identifier: String,
    pub name: String,
    /// `approve` (each agent write waits for a person) or `allow`.
    pub agent_writes: String,
    /// A webhook secret is kept for it.
    pub has_webhook_secret: bool,
    /// Who connected it (an account id), when that account is still there.
    pub linked_by: Option<String>,
    pub created_at: u64,
    /// When it was disconnected: nothing is read from the tracker for it since.
    pub unlinked_at: Option<u64>,
    /// When its rows go, unless it's connected again first.
    pub purge_after: Option<u64>,
    /// When the first read of its items finished.
    pub backfilled_at: Option<u64>,
    /// How many of its items the platform holds.
    pub items: u64,
    /// Agents' writes waiting for a person to approve.
    pub waiting_writes: u64,
}

/// `POST /api/projects/{project}/trackers`: connect a Plane project. A
/// project's owner or editor may. `project` is Plane's project id or its
/// identifier. Control reads the project with the key first: a key Plane
/// refuses or a project it doesn't know is a 422 and nothing is kept. The
/// key and the webhook secret are sealed and never come back.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConnectTracker {
    pub workspace_slug: String,
    pub project: String,
    pub api_key: String,
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

impl std::fmt::Debug for ConnectTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectTracker")
            .field("workspace_slug", &self.workspace_slug)
            .field("project", &self.project)
            .field("api_key", &"<redacted>")
            .field("webhook_secret", &self.webhook_secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// `PATCH /api/projects/{project}/trackers/{connection}`: whether agents'
/// writes to the tracker wait for a person (`approve`) or go (`allow`);
/// the answer is its [`TrackerConnection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerSettings {
    pub agent_writes: String,
}

/// `POST /api/projects/{project}/trackers/{connection}/writes`
/// ([`TrackerWrite`], answered with a [`TrackerWriteDone`]): a person's
/// write to a tracker, which lands at once. An owner or editor may.
pub const TRACKER_WRITES: &str = "/api/projects/{project}/trackers/{connection}/writes";

/// `GET /api/projects/{project}/tracker-writes`: the agents' writes waiting
/// for a person to approve ([`TrackerPendingWrites`]), for an owner or editor.
pub const PROJECT_TRACKER_WRITES: &str = "/api/projects/{project}/tracker-writes";

/// `PATCH /api/projects/{project}/tracker-writes/{write}` (a [`TrackerWrite`]
/// of the same kind and target: its new text), `POST …/approve` (send it)
/// and `POST …/drop`, each answered with a [`TrackerPendingWrite`]. A person's
/// alone: no machine or agent route reaches them.
pub const PROJECT_TRACKER_WRITE: &str = "/api/projects/{project}/tracker-writes/{write}";
pub const PROJECT_TRACKER_WRITE_APPROVE: &str = "/api/projects/{project}/tracker-writes/{write}/approve";
pub const PROJECT_TRACKER_WRITE_DROP: &str = "/api/projects/{project}/tracker-writes/{write}/drop";

/// `POST /api/daemon/projects/{project}/tracker-writes` ([`TrackerWrite`], answered with a
/// [`TrackerWriteDone`]): a machine's agent writes to a tracker, with `trackers.write` over the
/// project. It lands at once or waits for a person, as the connection says.
pub const AGENT_TRACKER_WRITES: &str = "/api/daemon/projects/{project}/tracker-writes";

/// `GET /api/daemon/trackers/reach`: the projects the machine holds
/// `trackers.write` over ([`TrackerReach`]).
pub const AGENT_TRACKER_REACH: &str = "/api/daemon/trackers/reach";

/// [`AGENT_TRACKER_WRITES`] for `project`.
pub fn agent_tracker_writes(project: &str) -> String {
    AGENT_TRACKER_WRITES.replace("{project}", project)
}

/// How a tracker item is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TrackerClose {
    Completed,
    Cancelled,
}

/// A write to a tracker's work item, the same for people and agents. `item`
/// is the mirror's `fi_` id, or `IDENT-N` (the connection's identifier and
/// the item's number). Assignees are not here: a tracker's members aren't
/// Arugula's people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackerWrite {
    /// A new item. `connection` is needed on an agent's route and, on a
    /// person's, is the path's. `state` is a state group (`backlog`,
    /// `unstarted`, `started`).
    Create {
        #[serde(default)]
        connection: Option<String>,
        title: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        state: Option<String>,
    },
    Comment {
        item: String,
        text: String,
    },
    Close {
        item: String,
        #[serde(rename = "as")]
        close_as: TrackerClose,
    },
    Reopen {
        item: String,
    },
    Edit {
        item: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
}

/// What a write came to: it landed (`sent`, with the item's `fi_` id), or
/// it waits for a person (`waiting`, with its `tw_` id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerWriteDone {
    /// `sent` or `waiting`.
    pub state: String,
    /// The waiting write's id (`tw_` and hex).
    pub write: Option<String>,
    /// The mirror row the write reached or made (`fi_` and hex), and its
    /// `IDENT-N`.
    pub item: Option<String>,
    pub reference: Option<String>,
    /// A sentence for whoever asked.
    pub text: String,
}

/// A write waiting for a person, as the Triage page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerPendingWrite {
    /// `tw_` and hex.
    pub id: String,
    pub connection: String,
    /// `waiting`, `sent`, `dropped` or `failed`.
    pub state: String,
    /// The write, as it will be sent (without the footer).
    pub write: TrackerWrite,
    /// `IDENT-N` of the item it targets, or none for a new item.
    pub target: Option<String>,
    /// Whose agent, and on which machine.
    pub person: String,
    pub machine: String,
    pub machine_name: String,
    /// Why it failed, in a sentence.
    pub error: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TrackerPendingWrites {
    pub project: String,
    pub writes: Vec<TrackerPendingWrite>,
}

/// The projects a machine holds `trackers.write` over, as its owner reads
/// them now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrackerReach {
    pub projects: Vec<TrackerReachProject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrackerReachProject {
    pub id: String,
    pub name: String,
}
