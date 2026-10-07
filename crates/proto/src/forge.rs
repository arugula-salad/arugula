//! A forge block (M36 to M40: a pull request or issue on Forgejo, GitHub or
//! GitLab) as the web client, the CLI and an agent see it: the block's
//! state (`GET /api/blocks/N`, `state`), what its methods take and answer,
//! and the normalized model the state is made of, the same whichever forge
//! it's on. The daemon (`forge/`) builds these, and its adapters map each
//! forge's answers onto the model; the web client draws the state with the
//! TypeScript made from [`ForgeState`].
//!
//! Some of these are also kept in the block's config (the drafts, the agent's
//! link, a new issue), so they read what an older daemon saved: no field is
//! required that an older one didn't write.

use serde::{Deserialize, Serialize};

use crate::{Gate, PaneId};

// ---------------------------------------------------------------- the model

/// Which forge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Forgejo,
    /// GitHub, or GitHub Enterprise (M38).
    Github,
    /// GitLab (M39): merge requests, through the person's `glab` login.
    Gitlab,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    #[default]
    Pr,
    /// M37.
    Issue,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    #[default]
    Open,
    Closed,
    Merged,
}

/// A side of a PR: which repository (a fork's, for its head), branch and
/// commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Branch {
    pub repo: Option<String>,
    pub branch: String,
    pub sha: String,
}

/// Who a review request is for: a person, or a team (`Org/Name`, or just
/// `Name` when the forge doesn't say).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Reviewer {
    User(String),
    Team(String),
}

impl Reviewer {
    pub fn name(&self) -> &str {
        match self {
            Reviewer::User(n) | Reviewer::Team(n) => n,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Item {
    pub kind: ItemKind,
    pub number: u64,
    pub url: String,
    pub title: String,
    pub body: String,
    /// The author's login.
    pub author: String,
    pub state: ItemState,
    pub draft: bool,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub base: Branch,
    pub head: Branch,
    /// `refs/pull/N/head`: what `diff` and `checkout` fetch (never the
    /// head's branch: AGit PRs have none).
    pub head_ref: String,
    pub merge_base: Option<String>,
    /// `None` while the forge works it out.
    pub mergeable: Option<bool>,
    /// The forge says branch protection blocks a merge (GitHub only;
    /// Forgejo can't tell, so false).
    pub blocked: bool,
    /// Review requests still pending (Forgejo's `requested_reviewers` also
    /// keeps whoever already reviewed: see `forge::forgejo`).
    pub requested: Vec<Reviewer>,
    pub comments: u64,
    pub updated_at: i64,
    pub merged_at: Option<i64>,
    pub merged_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    /// Started and not submitted (the reviewer's own draft).
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Review {
    pub id: String,
    pub author: Option<String>,
    pub state: ReviewState,
    pub commit: Option<String>,
    /// Made on an older head.
    pub stale: bool,
    pub at: Option<i64>,
    pub body: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CheckSource {
    /// A commit status (older CI, or another service).
    Status,
    /// Forgejo Actions, which write commit statuses.
    Action,
    /// A GitHub check run (Actions, or another app).
    CheckRun,
    /// A job in a GitLab merge request's head pipeline.
    PipelineJob,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Queued,
    Running,
    Success,
    Failure,
    Cancelled,
    Skipped,
    Neutral,
    ActionRequired,
    /// Waits for someone to start it (a GitLab manual job); not counted.
    Manual,
}

impl CheckState {
    pub fn red(self) -> bool {
        matches!(self, CheckState::Failure | CheckState::Cancelled | CheckState::ActionRequired)
    }
}

/// What a rerun would act on: an Actions run (its number in the repo) and
/// job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunRef {
    pub id: String,
    pub job: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Check {
    pub name: String,
    pub source: CheckSource,
    pub state: CheckState,
    pub allow_failure: bool,
    /// Absolute (Forgejo's own are relative to the site; the adapter makes
    /// them whole).
    pub url: Option<String>,
    pub description: Option<String>,
    pub run: Option<RunRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Commented,
    ReviewComment,
    Reviewed,
    ReviewRequested,
    ReviewRequestRemoved,
    ReviewDismissed,
    Pushed,
    Labeled,
    Assigned,
    Renamed,
    Referenced,
    Merged,
    Closed,
    Reopened,
    BranchDeleted,
    Milestone,
    /// GitHub says who was mentioned (`target`), not who wrote it.
    Mentioned,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Event {
    /// Provider-prefixed, stable across polls (`fj-1403`).
    pub id: String,
    pub at: i64,
    pub actor: Option<String>,
    pub kind: EventKind,
    /// The forge's own name for it, when it's `other` (or more exact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub what: Option<String>,
    /// Who a request is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub target: Option<Reviewer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub body: Option<String>,
    /// A push: how many commits, and whether it was forced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub commits: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub force: bool,
}

/// A pull request, read whole.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pr {
    pub item: Item,
    pub reviews: Vec<Review>,
    pub checks: Vec<Check>,
    pub rollup: Option<CheckState>,
    /// The newest events, oldest first.
    pub events: Vec<Event>,
}

/// M37: a pull request that refers to an issue (from the issue's
/// timeline), or one found by its head branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Linked {
    pub number: u64,
    pub title: String,
    pub state: ItemState,
    pub url: String,
    /// Its head branch, when the forge said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub head: Option<String>,
}

/// An issue, read whole (M37): the item (no branches), its newest events,
/// and the pull requests that refer to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Issue {
    pub item: Item,
    pub events: Vec<Event>,
    pub linked: Vec<Linked>,
}

// ------------------------------------------------- writes, drafts, the issue

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

/// A write to the forge: the methods `comment`, `review`, `merge`,
/// `rerun_checks` and `live`, with what each takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Write {
    Comment {
        body: String,
    },
    Review {
        event: ReviewEvent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        body: Option<String>,
    },
    Merge {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        style: Option<String>,
    },
    /// Run the failed checks again (GitLab: retry the head pipeline).
    #[serde(rename = "rerun_checks")]
    Rerun,
    /// M40: a webhook to this daemon on the repository, made or removed
    /// with the person's login (Forgejo, GitLab).
    Live {
        on: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    #[default]
    Waiting,
    Sent,
    Dropped,
}

/// An agent's write, waiting for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Draft {
    pub id: String,
    #[serde(flatten)]
    pub write: Write,
    /// Who drafted it (`mcp:claude-code`, `agent`).
    pub by: String,
    pub at_ms: u64,
    #[serde(default)]
    pub status: DraftStatus,
    /// Who sent or dropped it, and when.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub settled_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub settled_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    /// The last send failed: why (it waits again, text kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<String>,
}

/// The agent working on an issue, and what it made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AgentLink {
    pub branch: String,
    /// The worktree, absolute.
    pub worktree: String,
    /// What the branch was made from.
    pub base: String,
    /// The agent block.
    pub block: PaneId,
    /// `claude`, `codex`, …
    pub agent: String,
    pub at_ms: u64,
    /// Its pull request, once there is one (found once).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pr: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pr_url: Option<String>,
    /// The PR block opened beside the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pr_block: Option<PaneId>,
}

/// A new issue, before and after it reached the forge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewIssue {
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Who asked for it: a person, `mcp:<client>`, or `an agent`.
    #[serde(default)]
    pub by: String,
    /// An agent's: a draft, until a person sends it.
    #[serde(default)]
    pub agent: bool,
    #[serde(default)]
    pub at_ms: u64,
    #[serde(default)]
    pub status: DraftStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub settled_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    /// The last try failed: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<String>,
}

// ---------------------------------------------------------------- the state

/// What a PR or issue wants of you, as the client draws it: its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ForgeWantKind {
    Review,
    Failed,
    Changes,
    Mention,
    Done,
    Assigned,
}

impl ForgeWantKind {
    /// As it's written on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            ForgeWantKind::Review => "review",
            ForgeWantKind::Failed => "failed",
            ForgeWantKind::Changes => "changes",
            ForgeWantKind::Mention => "mention",
            ForgeWantKind::Done => "done",
            ForgeWantKind::Assigned => "assigned",
        }
    }
}

/// What it wants of you, as the client draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeWant {
    pub kind: ForgeWantKind,
    pub why: String,
}

/// A `tea` login to pick from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeLogin {
    pub name: String,
    pub url: String,
    pub user: String,
}

/// What the failed checks' rerun is: through the API (`api`: GitLab, as
/// `rerun_checks`), or on a forge with none (or no login), the run's page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Rerun {
    pub api: bool,
    pub url: Option<String>,
    pub note: String,
    /// GitHub: how many workflow runs it reruns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub runs: Option<u64>,
    /// GitLab: the pipeline's id, `null` when no check says. Absent
    /// (outer `None`) for the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(as = "Option<String>", optional = nullable))]
    pub pipeline: Option<Option<String>>,
}

/// GitHub's rate limit as last heard, and any backing off (M38).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RateLimit {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub used: Option<u64>,
    /// When the window resets (epoch seconds).
    pub reset: Option<u64>,
    pub requests: u64,
    pub not_modified: u64,
    /// Why it's polling slowly, if it is.
    pub backoff: Option<String>,
}

/// M40: how the block hears of changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ForgeLive {
    /// Pokes say when to read.
    Webhook,
    #[default]
    Polling,
}

/// A forge block's state: the PR or issue, what it wants of you, the agent's
/// drafts, and how it's kept current.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ForgeState {
    pub provider: Provider,
    /// M37: `pr` or `issue`.
    pub kind: ItemKind,
    pub repo: String,
    pub number: u64,
    pub api: Option<String>,
    pub login: Option<String>,
    pub host: Option<String>,
    pub dir: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    /// It can only read, and why (GitLab with no glab login: anonymous).
    pub read_only: Option<String>,
    /// Logins to pick from, when none (or several) matched.
    pub logins: Vec<ForgeLogin>,
    /// "You", on the forge.
    pub me: Option<String>,
    pub pr: Option<Pr>,
    /// M37: the issue, for `kind: issue`.
    pub issue: Option<Issue>,
    /// M37: the agent on it.
    pub link: Option<AgentLink>,
    /// M37: a new issue, before (and after) it went out.
    pub new: Option<NewIssue>,
    pub wants: Vec<ForgeWant>,
    /// Whether *Rerun checks* is possible here, and where the failed run
    /// is when it isn't (Forgejo).
    pub rerun: Option<Rerun>,
    pub drafts: Vec<Draft>,
    pub updated_ms: u64,
    pub polls: u64,
    /// M40: pokes heard (webhooks through control or straight here).
    pub pokes: u64,
    pub reads: u64,
    /// A client draws it.
    pub watching: bool,
    /// The last write's result (sent directly), for the client.
    pub said: Option<String>,
    /// The forge's rate limit and any backing off (M38: GitHub).
    pub rate: Option<RateLimit>,
    /// M40: webhook (pokes say when to read) or polling, and why.
    pub live: ForgeLive,
    /// Where the pokes come from, when there's a way for them to
    /// (`github-app`, `hook`).
    pub live_via: Option<String>,
    pub live_why: Option<String>,
    /// When it last heard from the webhook path.
    pub live_heard_ms: Option<u64>,
    /// A webhook this daemon made is on the repository.
    pub hook: bool,
}

// ------------------------------------------------------------- the methods

/// `refresh`: what it has read so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refreshed {
    pub reads: u64,
    pub polls: u64,
}

/// `login {name}`: the login it uses now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedIn {
    pub login: String,
}

/// `drafts`: an agent's writes, waiting and settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drafts {
    pub drafts: Vec<Draft>,
}

/// A write an agent asked for (`comment`, `review`, `merge`, `live` with
/// `agent: true` or from MCP): queued as a draft for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drafted {
    /// The draft's id.
    pub draft: String,
    pub status: DraftStatus,
    pub queued: u64,
    pub note: String,
}

/// A write a person asked for, gone out with the owner's login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    /// "a comment", "an approval", …
    pub sent: String,
    pub url: Option<String>,
    pub said: String,
    pub by: Option<String>,
    /// The review gate it approved (the rail's `allow`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
}

/// `diff`: the diff block opened for the PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diffed {
    pub block: PaneId,
    pub worktree: String,
    pub rev_a: String,
    pub rev_b: String,
}

/// `checkout`: the terminal opened in the PR's worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckedOut {
    pub pane: PaneId,
    pub worktree: String,
}

/// `agent`: the agent started on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStarted {
    pub agent: PaneId,
    pub branch: String,
    pub worktree: String,
    pub base: String,
}

#[cfg(test)]
mod wire {
    //! The state, the methods' answers and a write, against the `json!` the
    //! daemon built before they were types (literals copied from there): the
    //! web client reads these by name, so a key that moves or goes missing
    //! reads as `undefined`.

    use serde_json::{Value, json};

    use super::*;
    use crate::GateSource;

    fn branch(repo: Option<&str>, name: &str) -> Branch {
        Branch { repo: repo.map(str::to_owned), branch: name.into(), sha: format!("sha-{name}") }
    }

    fn item(kind: ItemKind, number: u64) -> Item {
        Item {
            kind,
            number,
            url: format!("https://git.example/o/r/{number}"),
            title: "A title".into(),
            body: "A body".into(),
            author: "wetneb".into(),
            state: ItemState::Open,
            draft: false,
            labels: vec!["bug".into()],
            assignees: vec!["jake".into()],
            base: branch(None, "main"),
            head: branch(Some("fork/r"), "topic"),
            head_ref: format!("refs/pull/{number}/head"),
            merge_base: Some("mb".into()),
            mergeable: Some(true),
            blocked: false,
            requested: vec![Reviewer::User("jake".into()), Reviewer::Team("Org/core".into())],
            comments: 3,
            updated_at: 1_790_975_391_548,
            merged_at: None,
            merged_by: None,
        }
    }

    fn item_json(kind: &str, number: u64) -> Value {
        json!({
            "kind": kind, "number": number, "url": format!("https://git.example/o/r/{number}"),
            "title": "A title", "body": "A body", "author": "wetneb", "state": "open", "draft": false,
            "labels": ["bug"], "assignees": ["jake"],
            "base": { "repo": null, "branch": "main", "sha": "sha-main" },
            "head": { "repo": "fork/r", "branch": "topic", "sha": "sha-topic" },
            "head_ref": format!("refs/pull/{number}/head"), "merge_base": "mb", "mergeable": true, "blocked": false,
            "requested": [{ "user": "jake" }, { "team": "Org/core" }], "comments": 3,
            "updated_at": 1_790_975_391_548_i64, "merged_at": null, "merged_by": null,
        })
    }

    /// The state of a block that has read nothing: what `create` builds,
    /// and what the daemon adds in `state()`.
    fn empty_json() -> Value {
        json!({
            "provider": "forgejo", "kind": "pr", "repo": "", "number": 0, "api": null, "login": null,
            "host": null, "dir": null, "loading": false, "error": null, "read_only": null, "logins": [],
            "me": null, "pr": null, "issue": null, "link": null, "new": null, "wants": [], "rerun": null,
            "drafts": [], "updated_ms": 0, "polls": 0, "pokes": 0, "reads": 0, "watching": false, "said": null,
            "rate": null, "live": "polling", "live_via": null, "live_why": null, "live_heard_ms": null,
            "hook": false,
        })
    }

    mod state {
        use super::*;

        #[test]
        fn nothing_read_yet() {
            assert_eq!(serde_json::to_value(ForgeState::default()).unwrap(), empty_json());
        }

        #[test]
        fn a_pr_read_whole() {
            let mut it = item(ItemKind::Pr, 7);
            it.state = ItemState::Merged;
            it.merged_at = Some(1_790_975_999_000);
            it.merged_by = Some("jake".into());
            let st = ForgeState {
                provider: Provider::Github,
                kind: ItemKind::Pr,
                repo: "o/r".into(),
                number: 7,
                api: Some("https://api.github.com".into()),
                login: Some("gh:github.com".into()),
                host: Some("github.com".into()),
                dir: Some("/home/a/r".into()),
                error: None,
                logins: vec![ForgeLogin { name: "t".into(), url: "https://git.example".into(), user: "me".into() }],
                me: Some("me".into()),
                pr: Some(Pr {
                    item: it,
                    reviews: vec![
                        Review {
                            id: "r1".into(),
                            author: Some("jake".into()),
                            state: ReviewState::ChangesRequested,
                            commit: Some("abc".into()),
                            stale: true,
                            at: Some(5),
                            body: Some("no".into()),
                            url: Some("https://git.example/r1".into()),
                        },
                        Review {
                            id: "r2".into(),
                            author: None,
                            state: ReviewState::Pending,
                            commit: None,
                            stale: false,
                            at: None,
                            body: None,
                            url: None,
                        },
                    ],
                    checks: vec![
                        Check {
                            name: "ci".into(),
                            source: CheckSource::CheckRun,
                            state: CheckState::Failure,
                            allow_failure: false,
                            url: Some("https://git.example/run/1".into()),
                            description: Some("it broke".into()),
                            run: Some(RunRef { id: "1".into(), job: Some("9".into()) }),
                        },
                        Check {
                            name: "lint".into(),
                            source: CheckSource::Status,
                            state: CheckState::Success,
                            allow_failure: true,
                            url: None,
                            description: None,
                            run: None,
                        },
                    ],
                    rollup: Some(CheckState::Failure),
                    events: vec![
                        Event {
                            id: "gh-1".into(),
                            at: 10,
                            actor: Some("jake".into()),
                            kind: EventKind::Pushed,
                            what: None,
                            target: None,
                            body: None,
                            commits: Some(2),
                            force: true,
                        },
                        Event {
                            id: "gh-2".into(),
                            at: 11,
                            actor: None,
                            kind: EventKind::ReviewRequested,
                            what: Some("x".into()),
                            target: Some(Reviewer::Team("Org/core".into())),
                            body: Some("hi".into()),
                            commits: None,
                            force: false,
                        },
                    ],
                }),
                wants: vec![
                    ForgeWant { kind: ForgeWantKind::Review, why: "review requested from you".into() },
                    ForgeWant { kind: ForgeWantKind::Failed, why: "1 check failed: ci".into() },
                ],
                rerun: Some(Rerun {
                    api: true,
                    url: Some("https://git.example/run/1".into()),
                    note: "n".into(),
                    runs: Some(1),
                    pipeline: None,
                }),
                drafts: vec![Draft {
                    id: "d1".into(),
                    write: Write::Comment { body: "text".into() },
                    by: "mcp:claude-code".into(),
                    at_ms: 99,
                    status: DraftStatus::Waiting,
                    settled_by: None,
                    settled_ms: None,
                    url: None,
                    error: None,
                }],
                updated_ms: 100,
                polls: 4,
                pokes: 1,
                reads: 2,
                watching: true,
                said: Some("sent".into()),
                rate: Some(RateLimit {
                    limit: Some(5000),
                    remaining: Some(90),
                    used: Some(4910),
                    reset: Some(1_790_000_000),
                    requests: 12,
                    not_modified: 3,
                    backoff: Some("low".into()),
                }),
                live: ForgeLive::Webhook,
                live_via: Some("github-app".into()),
                live_why: None,
                live_heard_ms: Some(98),
                hook: false,
                ..ForgeState::default()
            };
            let mut want = item_json("pr", 7);
            want["state"] = json!("merged");
            want["merged_at"] = json!(1_790_975_999_000_i64);
            want["merged_by"] = json!("jake");
            assert_eq!(
                serde_json::to_value(&st).unwrap(),
                json!({
                    "provider": "github", "kind": "pr", "repo": "o/r", "number": 7,
                    "api": "https://api.github.com", "login": "gh:github.com", "host": "github.com",
                    "dir": "/home/a/r", "loading": false, "error": null, "read_only": null,
                    "logins": [{ "name": "t", "url": "https://git.example", "user": "me" }],
                    "me": "me",
                    "pr": {
                        "item": want,
                        "reviews": [
                            { "id": "r1", "author": "jake", "state": "changes_requested", "commit": "abc",
                              "stale": true, "at": 5, "body": "no", "url": "https://git.example/r1" },
                            { "id": "r2", "author": null, "state": "pending", "commit": null,
                              "stale": false, "at": null, "body": null, "url": null },
                        ],
                        "checks": [
                            { "name": "ci", "source": "check_run", "state": "failure", "allow_failure": false,
                              "url": "https://git.example/run/1", "description": "it broke",
                              "run": { "id": "1", "job": "9" } },
                            { "name": "lint", "source": "status", "state": "success", "allow_failure": true,
                              "url": null, "description": null, "run": null },
                        ],
                        "rollup": "failure",
                        "events": [
                            { "id": "gh-1", "at": 10, "actor": "jake", "kind": "pushed", "commits": 2, "force": true },
                            { "id": "gh-2", "at": 11, "actor": null, "kind": "review_requested", "what": "x",
                              "target": { "team": "Org/core" }, "body": "hi" },
                        ],
                    },
                    "issue": null, "link": null, "new": null,
                    "wants": [
                        { "kind": "review", "why": "review requested from you" },
                        { "kind": "failed", "why": "1 check failed: ci" },
                    ],
                    "rerun": { "api": true, "url": "https://git.example/run/1", "runs": 1, "note": "n" },
                    "drafts": [{ "id": "d1", "method": "comment", "body": "text", "by": "mcp:claude-code",
                                 "at_ms": 99, "status": "waiting" }],
                    "updated_ms": 100, "polls": 4, "pokes": 1, "reads": 2, "watching": true, "said": "sent",
                    "rate": { "limit": 5000, "remaining": 90, "used": 4910, "reset": 1_790_000_000,
                              "requests": 12, "not_modified": 3, "backoff": "low" },
                    "live": "webhook", "live_via": "github-app", "live_why": null, "live_heard_ms": 98,
                    "hook": false,
                })
            );
        }

        #[test]
        fn an_issue_with_its_agent_and_a_new_one() {
            let st = ForgeState {
                kind: ItemKind::Issue,
                repo: "o/r".into(),
                number: 12,
                loading: true,
                issue: Some(Issue {
                    item: item(ItemKind::Issue, 12),
                    events: vec![],
                    linked: vec![
                        Linked {
                            number: 30,
                            title: "Fix".into(),
                            state: ItemState::Closed,
                            url: "https://git.example/o/r/30".into(),
                            head: Some("i12-fix".into()),
                        },
                        Linked {
                            number: 31,
                            title: "Other".into(),
                            state: ItemState::Open,
                            url: "https://git.example/o/r/31".into(),
                            head: None,
                        },
                    ],
                }),
                link: Some(AgentLink {
                    branch: "i12-fix".into(),
                    worktree: "/w".into(),
                    base: "main".into(),
                    block: 4,
                    agent: "claude".into(),
                    at_ms: 50,
                    pr: Some(30),
                    pr_url: Some("https://git.example/o/r/30".into()),
                    pr_block: Some(6),
                }),
                new: Some(NewIssue {
                    title: "T".into(),
                    body: "B".into(),
                    by: "mcp:x".into(),
                    agent: true,
                    at_ms: 60,
                    status: DraftStatus::Dropped,
                    settled_by: Some("jake".into()),
                    url: None,
                    error: Some("nope".into()),
                }),
                wants: vec![ForgeWant { kind: ForgeWantKind::Assigned, why: "assigned to you".into() }],
                rerun: Some(Rerun { api: true, url: None, note: "n".into(), runs: None, pipeline: Some(None) }),
                ..ForgeState::default()
            };
            let mut want = empty_json();
            let o = want.as_object_mut().unwrap();
            o.insert("kind".into(), json!("issue"));
            o.insert("repo".into(), json!("o/r"));
            o.insert("number".into(), json!(12));
            o.insert("loading".into(), json!(true));
            o.insert(
                "issue".into(),
                json!({
                    "item": item_json("issue", 12), "events": [],
                    "linked": [
                        { "number": 30, "title": "Fix", "state": "closed", "url": "https://git.example/o/r/30",
                          "head": "i12-fix" },
                        { "number": 31, "title": "Other", "state": "open", "url": "https://git.example/o/r/31" },
                    ],
                }),
            );
            o.insert(
                "link".into(),
                json!({ "branch": "i12-fix", "worktree": "/w", "base": "main", "block": 4, "agent": "claude",
                        "at_ms": 50, "pr": 30, "pr_url": "https://git.example/o/r/30", "pr_block": 6 }),
            );
            o.insert(
                "new".into(),
                json!({ "title": "T", "body": "B", "by": "mcp:x", "agent": true, "at_ms": 60,
                        "status": "dropped", "settled_by": "jake", "error": "nope" }),
            );
            o.insert("wants".into(), json!([{ "kind": "assigned", "why": "assigned to you" }]));
            o.insert("rerun".into(), json!({ "api": true, "url": null, "pipeline": null, "note": "n" }));
            assert_eq!(serde_json::to_value(&st).unwrap(), want);
        }

        /// The three shapes `rerun()` built, key by key: a GitHub run, a
        /// GitLab pipeline (named or not), and a page to open.
        #[test]
        fn the_rerun_in_each_provider() {
            let url = Some("https://git.example/run/1".to_owned());
            let github = Rerun {
                api: true,
                url: url.clone(),
                note: "Rerun reruns the failed jobs of each red workflow run".into(),
                runs: Some(2),
                pipeline: None,
            };
            assert_eq!(
                serde_json::to_value(github).unwrap(),
                json!({ "api": true, "url": "https://git.example/run/1", "runs": 2,
                    "note": "Rerun reruns the failed jobs of each red workflow run" })
            );
            let gitlab = |pipeline| Rerun {
                api: true,
                url: url.clone(),
                note: "Rerun retries the pipeline's failed jobs".into(),
                runs: None,
                pipeline: Some(pipeline),
            };
            assert_eq!(
                serde_json::to_value(gitlab(Some("77".into()))).unwrap(),
                json!({ "api": true, "url": "https://git.example/run/1", "pipeline": "77",
                    "note": "Rerun retries the pipeline's failed jobs" })
            );
            assert_eq!(
                serde_json::to_value(gitlab(None)).unwrap(),
                json!({ "api": true, "url": "https://git.example/run/1", "pipeline": null,
                    "note": "Rerun retries the pipeline's failed jobs" })
            );
            let page = Rerun { api: false, url: None, note: "on the run's page".into(), runs: None, pipeline: None };
            assert_eq!(
                serde_json::to_value(page).unwrap(),
                json!({ "api": false, "url": null, "note": "on the run's page" })
            );
        }

        #[test]
        fn the_drafts_of_every_write() {
            let d = |write| Draft {
                id: "d".into(),
                write,
                by: "an agent".into(),
                at_ms: 1,
                status: DraftStatus::Sent,
                settled_by: Some("jake".into()),
                settled_ms: Some(2),
                url: Some("u".into()),
                error: Some("e".into()),
            };
            let tail = json!({ "by": "an agent", "at_ms": 1, "status": "sent", "settled_by": "jake",
                "settled_ms": 2, "url": "u", "error": "e" });
            for (write, head) in [
                (Write::Comment { body: "b".into() }, json!({ "method": "comment", "body": "b" })),
                (
                    Write::Review { event: ReviewEvent::RequestChanges, body: Some("b".into()) },
                    json!({ "method": "review", "event": "request_changes", "body": "b" }),
                ),
                (
                    Write::Review { event: ReviewEvent::Approve, body: None },
                    json!({ "method": "review", "event": "approve" }),
                ),
                (Write::Merge { style: Some("squash".into()) }, json!({ "method": "merge", "style": "squash" })),
                (Write::Merge { style: None }, json!({ "method": "merge" })),
                (Write::Rerun, json!({ "method": "rerun_checks" })),
                (Write::Live { on: true }, json!({ "method": "live", "on": true })),
            ] {
                let mut want = json!({ "id": "d" });
                for (k, v) in head.as_object().unwrap().iter().chain(tail.as_object().unwrap()) {
                    want[k] = v.clone();
                }
                assert_eq!(serde_json::to_value(d(write)).unwrap(), want);
            }
        }
    }

    mod results {
        use super::*;

        #[test]
        fn what_each_method_answers() {
            fn to<T: serde::Serialize>(v: &T) -> Value {
                serde_json::to_value(v).unwrap()
            }
            assert_eq!(to(&Refreshed { reads: 3, polls: 9 }), json!({ "reads": 3, "polls": 9 }));
            assert_eq!(to(&LoggedIn { login: "t".into() }), json!({ "login": "t" }));
            let d = Draft {
                id: "d1".into(),
                write: Write::Merge { style: None },
                by: "an agent".into(),
                at_ms: 1,
                status: DraftStatus::Waiting,
                settled_by: None,
                settled_ms: None,
                url: None,
                error: None,
            };
            assert_eq!(
                to(&Drafts { drafts: vec![d] }),
                json!({ "drafts": [{ "id": "d1", "method": "merge", "by": "an agent", "at_ms": 1, "status": "waiting" }] })
            );
            assert_eq!(to(&Drafts { drafts: vec![] }), json!({ "drafts": [] }));
            assert_eq!(
                to(&Drafted {
                    draft: "d2".into(),
                    status: DraftStatus::Waiting,
                    queued: 2,
                    note: "a person sends, edits or drops it".into(),
                }),
                json!({ "draft": "d2", "status": "waiting", "queued": 2, "note": "a person sends, edits or drops it" })
            );
            assert_eq!(
                to(&Diffed { block: 8, worktree: "/w".into(), rev_a: "mb".into(), rev_b: "refs/arugula/pr/7".into() }),
                json!({ "block": 8, "worktree": "/w", "rev_a": "mb", "rev_b": "refs/arugula/pr/7" })
            );
            assert_eq!(to(&CheckedOut { pane: 9, worktree: "/w".into() }), json!({ "pane": 9, "worktree": "/w" }));
            assert_eq!(
                to(&AgentStarted { agent: 5, branch: "i1-x".into(), worktree: "/w".into(), base: "main".into() }),
                json!({ "agent": 5, "branch": "i1-x", "worktree": "/w", "base": "main" })
            );
        }

        #[test]
        fn a_write_that_went_out() {
            let w = Written { sent: "a comment".into(), url: None, said: "ok".into(), by: None, gate: None };
            assert_eq!(
                serde_json::to_value(w).unwrap(),
                json!({ "sent": "a comment", "url": null, "said": "ok", "by": null })
            );
            let gate = Gate {
                member: "m".into(),
                op: "o".into(),
                gate: "g".into(),
                env: None,
                since: None,
                expires: None,
                approvals: 0,
                needed: 1,
                command: None,
                source: GateSource::Forge { api: "https://git.example/api/v1".into(), url: "u".into(), number: 7 },
            };
            let w = Written {
                sent: "an approval".into(),
                url: Some("u".into()),
                said: "ok".into(),
                by: Some("jake".into()),
                gate: Some(gate.clone()),
            };
            assert_eq!(
                serde_json::to_value(w).unwrap(),
                json!({ "sent": "an approval", "url": "u", "said": "ok", "by": "jake", "gate": gate })
            );
        }
    }

    mod write {
        use super::*;

        #[test]
        fn each_method_as_it_was_logged_and_drafted() {
            for (w, j) in [
                (Write::Comment { body: "b".into() }, json!({ "method": "comment", "body": "b" })),
                (
                    Write::Review { event: ReviewEvent::Comment, body: None },
                    json!({ "method": "review", "event": "comment" }),
                ),
                (
                    Write::Review { event: ReviewEvent::Approve, body: Some("lgtm".into()) },
                    json!({ "method": "review", "event": "approve", "body": "lgtm" }),
                ),
                (Write::Merge { style: None }, json!({ "method": "merge" })),
                (Write::Merge { style: Some("rebase".into()) }, json!({ "method": "merge", "style": "rebase" })),
                (Write::Rerun, json!({ "method": "rerun_checks" })),
                (Write::Live { on: false }, json!({ "method": "live", "on": false })),
            ] {
                assert_eq!(serde_json::to_value(&w).unwrap(), j);
                assert_eq!(serde_json::from_value::<Write>(j).unwrap(), w);
            }
        }
    }

    /// Block state is kept in the block's config: what an older daemon saved
    /// still reads (no `status`, no settled fields, `null`s).
    mod saved {
        use super::*;

        #[test]
        fn an_older_draft_a_link_and_a_new_issue() {
            let d: Draft = serde_json::from_value(
                json!({ "id": "d1", "method": "review", "event": "approve", "body": null, "by": "an agent",
                    "at_ms": 5, "settled_by": null, "url": null, "error": null }),
            )
            .unwrap();
            assert_eq!(d.status, DraftStatus::Waiting);
            assert_eq!(d.write, Write::Review { event: ReviewEvent::Approve, body: None });
            let l: AgentLink = serde_json::from_value(
                json!({ "branch": "b", "worktree": "/w", "base": "main", "block": 3, "agent": "claude",
                    "at_ms": 1, "pr": null, "pr_url": null, "pr_block": null }),
            )
            .unwrap();
            assert_eq!(l.pr, None);
            let n: NewIssue = serde_json::from_value(json!({ "title": "T" })).unwrap();
            assert_eq!(n, NewIssue { title: "T".into(), ..NewIssue::default() });
            let n: NewIssue = serde_json::from_value(
                json!({ "title": "T", "agent": true, "status": "waiting", "settled_by": null, "error": null }),
            )
            .unwrap();
            assert!(n.agent && n.status == DraftStatus::Waiting);
        }
    }
}
