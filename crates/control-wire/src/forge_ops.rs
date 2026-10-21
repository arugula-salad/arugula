//! Typed forge operations (M169, #1215): reads, drafts and sends on a
//! linked repo's PRs and issues, by name and version. None needs a block
//! open, or this machine awake.
//!
//! One route takes them all: `POST /api/forge/ops` with a [`Call`], the
//! operation's name (`op`), the version its caller was written for (`v`)
//! and its arguments beside them. It's answered with an [`Answer`]: the
//! same `op` and `v`, and the `result`. The API, MCP and the CLI all call
//! it, so each operation is written once, on the platform.
//!
//! **Versions.** Each operation has its own ([`Op::version`]); all are 1
//! now. A change that would break a caller makes a new version, and the
//! platform keeps answering the old one. A call for a version the platform
//! doesn't speak is a 400 that says which it does. Nothing else on
//! control's API is versioned: this is new with these.
//!
//! **Who may.** Reads and drafts: a person (the page, the CLI), or a
//! machine with a live `cloud.drive` grant for its owner, each through
//! M165's viewer check, as the routes they're built on. Sends
//! ([`Op::sends`]) are a person's only: "agents draft, people send"
//! (aru-041), so a machine is refused (403, `agents_draft`). Sending is
//! M168's: until it's built, a send is a 501 with `reason: "not_yet"`.
//! MCP lists no send.
//!
//! `inbox.list` is M171's, which adds it here.

use serde::{Deserialize, Serialize};

use crate::projects::{ForgeDraft, ForgeEvent, ForgeItem, NewDraft};

/// `POST`: a [`Call`], answered with an [`Answer`].
pub const FORGE_OPS: &str = "/api/forge/ops";

/// A send's refusal until M168: the `reason` beside the sentence.
pub const NOT_YET: &str = "not_yet";
/// A machine asked to send.
pub const AGENTS_DRAFT: &str = "agents_draft";

/// An operation and the version its caller speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    #[serde(flatten)]
    pub op: Op,
    pub v: u32,
}

impl Call {
    /// The operation at the version this build speaks.
    pub fn new(op: Op) -> Self {
        let v = op.version();
        Self { op, v }
    }
}

/// Every operation, by its name on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    /// A PR: [`ItemView`].
    #[serde(rename = "pr.get")]
    PrGet(ItemRef),
    /// An issue: [`ItemView`].
    #[serde(rename = "issue.get")]
    IssueGet(ItemRef),
    /// A project's PRs and issues, or one repo's:
    /// [`ForgeItems`](crate::projects::ForgeItems).
    #[serde(rename = "item.list")]
    ItemList(ItemQuery),
    /// A PR's head commit's checks: [`ForgeChecks`](crate::projects::ForgeChecks).
    #[serde(rename = "checks.get")]
    ChecksGet(ItemRef),
    /// A comment, a review or a new issue, waiting for a person:
    /// [`DraftCreated`](crate::projects::DraftCreated).
    #[serde(rename = "draft.create")]
    DraftCreate(NewDraft),
    /// Drafts: [`ForgeDrafts`](crate::projects::ForgeDrafts).
    #[serde(rename = "draft.list")]
    DraftList(DraftQuery),
    /// One draft: [`ForgeDraft`].
    #[serde(rename = "draft.get")]
    DraftGet(DraftRef),
    /// [`DraftDiscarded`](crate::projects::DraftDiscarded).
    #[serde(rename = "draft.discard")]
    DraftDiscard(DraftRef),
    /// Send a waiting draft, as the person: [`Sent`].
    #[serde(rename = "draft.send")]
    DraftSend(DraftRef),
    /// Comment on a PR, as the person: [`Sent`].
    #[serde(rename = "pr.comment")]
    PrComment(CommentOn),
    /// Review a PR, as the person: [`Sent`].
    #[serde(rename = "pr.review")]
    PrReview(ReviewOf),
    /// Open an issue, as the person: [`Sent`].
    #[serde(rename = "issue.create")]
    IssueCreate(NewIssue),
    /// Comment on an issue, as the person: [`Sent`].
    #[serde(rename = "issue.comment")]
    IssueComment(CommentOn),
}

/// Every operation's name, reads and drafts first.
pub const NAMES: [&str; 13] = [
    "pr.get",
    "issue.get",
    "item.list",
    "checks.get",
    "draft.create",
    "draft.list",
    "draft.get",
    "draft.discard",
    "draft.send",
    "pr.comment",
    "pr.review",
    "issue.create",
    "issue.comment",
];

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::PrGet(_) => "pr.get",
            Op::IssueGet(_) => "issue.get",
            Op::ItemList(_) => "item.list",
            Op::ChecksGet(_) => "checks.get",
            Op::DraftCreate(_) => "draft.create",
            Op::DraftList(_) => "draft.list",
            Op::DraftGet(_) => "draft.get",
            Op::DraftDiscard(_) => "draft.discard",
            Op::DraftSend(_) => "draft.send",
            Op::PrComment(_) => "pr.comment",
            Op::PrReview(_) => "pr.review",
            Op::IssueCreate(_) => "issue.create",
            Op::IssueComment(_) => "issue.comment",
        }
    }

    /// The version of it this build speaks.
    pub fn version(&self) -> u32 {
        1
    }

    /// Whether it writes to GitHub: a person's action only.
    pub fn sends(&self) -> bool {
        matches!(self, Op::DraftSend(_) | Op::PrComment(_) | Op::PrReview(_) | Op::IssueCreate(_) | Op::IssueComment(_))
    }
}

/// A PR or an issue on a linked repo: `OWNER/NAME` and its number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRef {
    pub repo: String,
    pub number: u64,
}

impl ItemRef {
    /// `OWNER/NAME#N`, or its link on github.com (`…/pull/N`,
    /// `…/issues/N`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (repo, n) = match s.split_once("://") {
            Some((_, rest)) => {
                let mut p = rest.split(['/', '?', '#']).filter(|x| !x.is_empty());
                let host = p.next()?.to_ascii_lowercase();
                let (o, r, what, n) = (p.next()?, p.next()?, p.next()?, p.next()?);
                if !matches!(host.as_str(), "github.com" | "www.github.com") || !matches!(what, "pull" | "issues") {
                    return None;
                }
                (format!("{o}/{r}"), n)
            }
            None => {
                let (repo, n) = s.rsplit_once('#')?;
                (repo.to_owned(), n)
            }
        };
        let ok = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        let number = n.parse().ok().filter(|n| *n > 0)?;
        match repo.split_once('/') {
            Some((o, r)) if ok(o) && ok(r) => Some(Self { repo, number }),
            _ => None,
        }
    }
}

impl std::fmt::Display for ItemRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// `item.list`: a project's items (the reader's personal project when
/// neither `project` nor `repo` says), or one linked repo's. The rest
/// narrows it as `GET /api/projects/{project}/items`'s query does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemQuery {
    #[serde(default)]
    pub project: Option<String>,
    /// `OWNER/NAME`.
    #[serde(default)]
    pub repo: Option<String>,
    /// `pr` or `issue`.
    #[serde(default)]
    pub kind: Option<String>,
    /// `open`, `closed` or `merged`.
    #[serde(default)]
    pub state: Option<String>,
    /// A GitHub login.
    #[serde(default)]
    pub author: Option<String>,
    /// A GitHub login among the requested reviewers.
    #[serde(default)]
    pub review_requested: Option<String>,
    /// Text in the title.
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// `draft.list`: a project's drafts, a repo's (and an item's, with
/// `number`), or with neither the reader's own across their projects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftQuery {
    #[serde(default)]
    pub project: Option<String>,
    /// `OWNER/NAME`.
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub number: Option<u64>,
    /// Only the reader's own.
    #[serde(default)]
    pub mine: bool,
    /// `waiting` (the default), `sent`, `discarded`, `failed` or `all`.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// A draft, by its id (`dft_…`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRef {
    pub draft: String,
}

/// `pr.comment` and `issue.comment`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentOn {
    pub repo: String,
    pub number: u64,
    pub body: String,
}

/// `pr.review`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOf {
    pub repo: String,
    pub number: u64,
    /// `COMMENT`, `APPROVE` or `REQUEST_CHANGES`.
    pub event: String,
    #[serde(default)]
    pub body: String,
}

/// `issue.create`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewIssue {
    pub repo: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// What every operation answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer<T> {
    pub op: String,
    pub v: u32,
    pub result: T,
}

/// `pr.get` and `issue.get`: the item, what happened to it, and its
/// waiting drafts. Metadata, as the mirror keeps it: bodies are M166's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemView {
    pub item: ForgeItem,
    pub events: Vec<ForgeEvent>,
    pub drafts: Vec<ForgeDraft>,
}

/// What a send answers (M168): the comment, review or issue on GitHub,
/// and the draft it sent, if it was one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub draft: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The name, the version and the arguments sit side by side.
    #[test]
    fn calls_on_the_wire() {
        let c = Call::new(Op::PrGet(ItemRef { repo: "acme/app".into(), number: 7 }));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({ "op": "pr.get", "v": 1, "repo": "acme/app", "number": 7 }));
        assert_eq!(serde_json::from_value::<Call>(v).unwrap(), c);
        let d: Call = serde_json::from_value(json!({ "op": "draft.create", "v": 1, "repo": "acme/app",
            "number": 7, "kind": "review", "event": "APPROVE" }))
        .unwrap();
        let Op::DraftCreate(n) = &d.op else { panic!("{d:?}") };
        assert_eq!((n.kind.as_str(), n.number, n.body.as_str()), ("review", Some(7), ""));
        let l: Call = serde_json::from_value(json!({ "op": "item.list", "v": 1 })).unwrap();
        assert_eq!(l.op, Op::ItemList(ItemQuery::default()));
        assert!(serde_json::from_value::<Call>(json!({ "op": "pr.merge", "v": 1 })).is_err());
        assert!(serde_json::from_value::<Call>(json!({ "op": "pr.get", "repo": "a/b", "number": 1 })).is_err());
    }

    #[test]
    fn item_refs() {
        let it = ItemRef { repo: "acme/app".into(), number: 7 };
        assert_eq!(it.to_string(), "acme/app#7");
        let want = Some(it);
        assert_eq!(ItemRef::parse("acme/app#7"), want);
        assert_eq!(ItemRef::parse(" https://github.com/acme/app/pull/7/files "), want);
        assert_eq!(ItemRef::parse("https://github.com/acme/app/issues/7#issuecomment-1"), want);
        for bad in ["acme/app", "acme#7", "acme/app#0", "acme/app#x", "a/b/c#1", "https://gitlab.com/a/b/pull/1", "%7"]
        {
            assert_eq!(ItemRef::parse(bad), None, "{bad}");
        }
    }

    /// Every operation's name is in [`NAMES`], and only the five writes to
    /// GitHub send.
    #[test]
    fn names_and_sends() {
        let r = ItemRef::default;
        let d = DraftRef::default;
        let all = [
            Op::PrGet(r()),
            Op::IssueGet(r()),
            Op::ItemList(ItemQuery::default()),
            Op::ChecksGet(r()),
            Op::DraftCreate(NewDraft::default()),
            Op::DraftList(DraftQuery::default()),
            Op::DraftGet(d()),
            Op::DraftDiscard(d()),
            Op::DraftSend(d()),
            Op::PrComment(CommentOn::default()),
            Op::PrReview(ReviewOf::default()),
            Op::IssueCreate(NewIssue::default()),
            Op::IssueComment(CommentOn::default()),
        ];
        assert_eq!(all.iter().map(Op::name).collect::<Vec<_>>(), NAMES);
        let sends: Vec<&str> = all.iter().filter(|o| o.sends()).map(Op::name).collect();
        assert_eq!(sends, ["draft.send", "pr.comment", "pr.review", "issue.create", "issue.comment"]);
        for o in &all {
            let v = serde_json::to_value(Call::new(o.clone())).unwrap();
            assert_eq!(v["op"], o.name());
        }
    }
}
