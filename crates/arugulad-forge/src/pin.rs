//! M172 (#1218): a PR or issue block on a GitHub repo linked to a project
//! in control is a pin onto the platform's item, and this machine stops
//! reading GitHub for it ("The daemon's forge reader stays for unlinked
//! repos; linked repos' blocks are pins", aru-087).
//!
//! - **Becoming one.** When a github.com block reads, it asks the platform
//!   whether its repo is linked for the owner ([`platform::link`], kept a
//!   few minutes). If it is, the item's id (`fi_…`) is found with `pr.get`
//!   or `issue.get` (`POST /api/forge/ops`), and the block keeps the pin
//!   (`{kind, global_id}`) in its config: after a restart it asks the
//!   platform first, not GitHub. An item the mirror doesn't have yet is
//!   read from GitHub, and looked for again in a few minutes.
//! - **Reading.** The item, its checks (a PR's) and its events (`GET
//!   /api/forge/items/{id}`, `…/checks`, `…/events`), as this machine for
//!   its owner, through M165's viewer check, mapped onto the reader's model
//!   so the block's attention, text and drafts work as they do on GitHub's
//!   answers. What the mirror keeps is metadata, so the text comes from
//!   `GET …/content` (M166): the description, the comments, reviews' text
//!   and review comments. It can cost the platform a GitHub call, so it's
//!   read when the block first reads and again when the events show
//!   something new (their count or newest id changed), and what it said
//!   last is kept otherwise ([`Kept`]). When it can't be read (not found,
//!   refused, unreachable, an older control without the route) the block
//!   shows the metadata alone, as before, with no error in place of the
//!   text, and asks again when something is new or after [`LOOK_AGAIN`].
//!   There are still no PRs that refer to an issue. "You" is the owner's
//!   GitHub login as the platform has it, without their teams.
//! - **Falling back.** The platform says it isn't linked (or the item is
//!   gone): the pin goes and the block reads GitHub again. The platform
//!   can't be reached: what it said last stays, marked stale, for
//!   [`STALE`]; with nothing to show yet (a restart), or after that, the
//!   block reads GitHub until the platform answers again. A block is never
//!   blank because the platform is down.
//!
//! Writes a person sends from a pinned block still go out with their `gh`
//! (as M170 leaves them) until sending is the platform's (M168), and *Diff*
//! and *Checkout* still fetch with the person's git.

use std::time::{Duration, Instant};

use arugula_control_wire::{
    forge_ops::{Answer, Call, ItemRef, ItemView, Op},
    projects::{ForgeCheck, ForgeChecks, ForgeContent, ForgeEvent, ForgeEvents, ForgeItem, ForgeItems},
};
use arugula_proto::forge::Rerun;
use reqwest::Method;
use serde_json::json;

use super::{
    model::{self, Event, EventKind, Issue, Item, ItemKind, ItemState, Linked, Pr, Review, ReviewState, Reviewer},
    platform::{self, Failed, Link},
};

/// How long what the platform said last is shown while it can't be
/// reached, before the block reads GitHub instead.
pub const STALE: Duration = Duration::from_secs(600);
/// How long before an item the mirror didn't have is looked for again.
pub const LOOK_AGAIN: Duration = Duration::from_secs(300);

/// What the platform has of a pinned item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub item: ForgeItem,
    pub checks: Vec<ForgeCheck>,
    pub events: Vec<ForgeEvent>,
    /// The text, when the platform gave it (now, or kept from before).
    pub content: Option<ForgeContent>,
}

/// What the last `/content` read left, so the next poll can tell whether
/// to ask again.
#[derive(Debug, Clone)]
pub struct Kept {
    id: String,
    /// The events it was asked at: how many, and the newest one's id.
    seen: (usize, Option<String>),
    /// What it said; `None` if it couldn't.
    content: Option<ForgeContent>,
    at: Instant,
}

impl Kept {
    /// Whether `/content` is asked again: it never was for this item, the
    /// events changed, or it failed and [`LOOK_AGAIN`] has passed.
    fn stale(&self, id: &str, seen: &(usize, Option<String>)) -> bool {
        self.id != id || self.seen != *seen || (self.content.is_none() && self.at.elapsed() >= LOOK_AGAIN)
    }
}

/// The mirror's id for `repo#number`, when it has it as a `kind`: `None`
/// when it doesn't (not linked for the owner, not mirrored yet, or a PR
/// where an issue was asked for), or why the platform couldn't say.
pub async fn resolve(repo: &str, number: u64, kind: ItemKind) -> Result<Option<String>, String> {
    let control = super::live::control().ok_or("not in Arugula control")?;
    let at = ItemRef { repo: repo.to_owned(), number };
    let op = match kind {
        ItemKind::Pr => Op::PrGet(at),
        ItemKind::Issue => Op::IssueGet(at),
    };
    let body = serde_json::to_value(Call::new(op)).map_err(|e| e.to_string())?;
    match control.cloud_now(Method::POST, arugula_control_wire::forge_ops::FORGE_OPS, Some(&body)).await {
        Ok(v) => {
            let a: Answer<ItemView> =
                serde_json::from_value(v).map_err(|e| format!("the platform's answer to {}: {e}", a_name(kind)))?;
            Ok(Some(a.result.item.id).filter(|id| good_id(id)))
        }
        Err(e) => match platform::failed(e) {
            // Not linked, not readable or not in the mirror: GitHub's, for now.
            Failed::NotLinked | Failed::Refused(_) => Ok(None),
            Failed::Later(why) => Err(why),
        },
    }
}

fn a_name(kind: ItemKind) -> &'static str {
    if kind == ItemKind::Pr { "pr.get" } else { "issue.get" }
}

/// An id as the platform makes them (`fi_` and hex), so nothing else
/// reaches its paths.
pub fn good_id(id: &str) -> bool {
    id.strip_prefix("fi_").is_some_and(|h| !h.is_empty() && h.len() <= 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))
}

/// The item, its checks (a PR's) and its events, and its text: asked of
/// `/content` only when `kept` (what the last read left) doesn't cover the
/// events now. A `/content` that fails is no failure of the read.
pub async fn read(id: &str, kind: ItemKind, kept: Option<&Kept>) -> Result<(Read, Kept), Failed> {
    if !good_id(id) {
        return Err(Failed::NotLinked);
    }
    let control = super::live::control().ok_or(Failed::NotLinked)?;
    let get = |path: String| {
        let control = control.clone();
        async move { control.cloud_now(Method::GET, &path, None).await.map_err(platform::failed) }
    };
    let parse = |what: &str, e: serde_json::Error| Failed::Later(format!("the platform's {what}: {e}"));
    let item: ForgeItem =
        serde_json::from_value(get(format!("/api/forge/items/{id}")).await?).map_err(|e| parse("item", e))?;
    let checks = if kind == ItemKind::Pr {
        let c: ForgeChecks = serde_json::from_value(get(format!("/api/forge/items/{id}/checks")).await?)
            .map_err(|e| parse("checks", e))?;
        c.checks
    } else {
        vec![]
    };
    let events: ForgeEvents =
        serde_json::from_value(get(format!("/api/forge/items/{id}/events")).await?).map_err(|e| parse("events", e))?;
    let seen = (events.events.len(), events.events.last().map(|e| e.id.clone()));
    let kept = match kept {
        Some(k) if !k.stale(id, &seen) => k.clone(),
        _ => {
            let content = get(format!("/api/forge/items/{id}/content"))
                .await
                .ok()
                .and_then(|v| serde_json::from_value::<ForgeContent>(v).ok());
            Kept { id: id.to_owned(), seen, content, at: Instant::now() }
        }
    };
    Ok((Read { item, checks, events: events.events, content: kept.content.clone() }, kept))
}

/// An open PR from `branch` in the repo, by the mirror (an issue an agent
/// works on waits for one, M37).
pub async fn pr_by_head(link: &Link, repo: &str, branch: &str) -> Result<Option<Linked>, Failed> {
    let control = super::live::control().ok_or(Failed::NotLinked)?;
    let path = format!("/api/projects/{}/items?repo={repo}&kind=pr&state=open", link.project);
    let v = control.cloud_now(Method::GET, &path, None).await.map_err(platform::failed)?;
    let items: ForgeItems =
        serde_json::from_value(v).map_err(|e| Failed::Later(format!("the platform's items: {e}")))?;
    Ok(items.items.into_iter().filter(|i| i.head_ref.as_deref() == Some(branch)).max_by_key(|i| i.number).map(|i| {
        Linked {
            number: i.number,
            url: url(repo, "pr", i.number),
            title: i.title,
            state: state(&i.state),
            head: i.head_ref,
        }
    }))
}

/// `OWNER/NAME#N` on github.com.
fn url(repo: &str, kind: &str, number: u64) -> String {
    let path = if kind == "issue" { "issues" } else { "pull" };
    format!("https://github.com/{repo}/{path}/{number}")
}

fn state(s: &str) -> ItemState {
    match s {
        "merged" => ItemState::Merged,
        "closed" => ItemState::Closed,
        _ => ItemState::Open,
    }
}

/// The item as the reader's model has it (the module's docs say what the
/// mirror doesn't keep).
fn item(r: &Read) -> Item {
    let i = &r.item;
    let comments =
        events(r).iter().filter(|e| matches!(e.kind, EventKind::Commented | EventKind::ReviewComment)).count();
    let mut item = Item {
        kind: if i.kind == "issue" { ItemKind::Issue } else { ItemKind::Pr },
        number: i.number,
        url: url(&i.repo, &i.kind, i.number),
        title: i.title.clone(),
        body: r.content.as_ref().and_then(|c| c.body.as_ref()).map(|b| b.body.clone()).unwrap_or_default(),
        author: i.author.clone().unwrap_or_default(),
        state: state(&i.state),
        draft: i.draft,
        labels: i.labels.clone(),
        assignees: i.assignees.clone(),
        head_ref: if i.kind == "pr" { format!("refs/pull/{}/head", i.number) } else { String::new() },
        // GitHub's `mergeable_state`: a conflict is `dirty`, and `unknown`
        // (or none yet) is still being worked out.
        mergeable: match i.merge_state.as_deref() {
            Some("dirty") => Some(false),
            None | Some("unknown") => None,
            Some(_) => Some(true),
        },
        blocked: i.merge_state.as_deref() == Some("blocked"),
        // A team is `org/slug`, which no login can be.
        requested: i
            .reviewers
            .iter()
            .map(|r| if r.contains('/') { Reviewer::Team(r.clone()) } else { Reviewer::User(r.clone()) })
            .collect(),
        comments: comments as u64,
        updated_at: i.updated_at as i64,
        ..Default::default()
    };
    item.head.sha = i.head_sha.clone().unwrap_or_default();
    item.head.branch = i.head_ref.clone().unwrap_or_default();
    item.base.branch = i.base_ref.clone().unwrap_or_default();
    item
}

fn review_state(s: Option<&str>) -> ReviewState {
    match s {
        Some("approved") => ReviewState::Approved,
        Some("changes_requested") => ReviewState::ChangesRequested,
        Some("dismissed") => ReviewState::Dismissed,
        _ => ReviewState::Commented,
    }
}

/// A review's text, when `/content` has it: the review of that author at
/// that time (the mirror's event ids aren't GitHub's).
fn review_text(r: &Read, e: &ForgeEvent) -> Option<String> {
    let author = e.actor.as_deref()?;
    r.content.as_ref()?.comments.iter().find_map(|t| {
        (t.kind == "review"
            && t.created_at == e.at
            && t.author.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(author))
            && !t.body.is_empty())
        .then(|| t.body.clone())
    })
}

/// The mirror's events in the timeline's words: oldest first, as GitHub's.
/// With `/content`, comments and review comments are its (with their
/// text) instead of the mirror's rows that one was made.
fn events(r: &Read) -> Vec<Event> {
    let texts = r.content.is_some();
    let mut out: Vec<Event> = r
        .events
        .iter()
        .filter(|e| !(texts && matches!(e.kind.as_str(), "comment" | "review_comment")))
        .map(|e| {
            let (kind, what) = match e.kind.as_str() {
                "comment" => (EventKind::Commented, None),
                "review_comment" => (EventKind::ReviewComment, None),
                "review" => (EventKind::Reviewed, e.review_state.clone()),
                "merged" => (EventKind::Merged, None),
                "closed" => (EventKind::Closed, None),
                "reopened" => (EventKind::Reopened, None),
                "review_requested" => (EventKind::ReviewRequested, None),
                "review_request_removed" => (EventKind::ReviewRequestRemoved, None),
                "opened" => (EventKind::Other, Some("opened it".to_owned())),
                "ready_for_review" => (EventKind::Other, Some("marked it ready for review".to_owned())),
                "converted_to_draft" => (EventKind::Other, Some("made it a draft".to_owned())),
                k => (EventKind::Other, Some(k.replace('_', " "))),
            };
            Event {
                id: format!("pf-{}", e.id),
                at: e.at as i64,
                actor: e.actor.clone(),
                kind,
                what,
                target: None,
                body: if kind == EventKind::Reviewed { review_text(r, e) } else { None },
                commits: None,
                force: false,
            }
        })
        .collect();
    for t in r.content.iter().flat_map(|c| &c.comments) {
        let kind = match t.kind.as_str() {
            "comment" => EventKind::Commented,
            "review_comment" => EventKind::ReviewComment,
            _ => continue,
        };
        out.push(Event {
            id: format!("gh-{}", t.id),
            at: t.created_at as i64,
            actor: t.author.clone(),
            kind,
            what: None,
            target: None,
            body: Some(t.body.clone()),
            commits: None,
            force: false,
        });
    }
    out.sort_by_key(|e| e.at);
    out
}

/// A pinned PR, as the block shows it.
pub fn pr(r: &Read) -> Pr {
    let runs: Vec<_> = r
        .checks
        .iter()
        .filter(|c| c.kind == "check_run")
        .map(|c| json!({ "name": c.name, "status": c.status, "conclusion": c.conclusion, "html_url": c.url }))
        .collect();
    let statuses: Vec<_> = r
        .checks
        .iter()
        .filter(|c| c.kind == "status")
        .map(|c| {
            let state = if c.status == "pending" { "pending" } else { c.conclusion.as_deref().unwrap_or("") };
            json!({ "context": c.name, "state": state, "target_url": c.url })
        })
        .collect();
    let checks = arugula_forge_github::parse::checks(&json!({ "check_runs": runs }), &json!({ "statuses": statuses }));
    let reviews = r
        .events
        .iter()
        .filter(|e| e.kind == "review")
        .map(|e| Review {
            id: format!("pf-{}", e.id),
            author: e.actor.clone(),
            state: review_state(e.review_state.as_deref()),
            commit: None,
            stale: false,
            at: Some(e.at as i64),
            body: review_text(r, e),
            url: None,
        })
        .collect();
    Pr { item: item(r), reviews, rollup: model::rollup(&checks), checks, events: events(r) }
}

/// A pinned issue, as the block shows it.
pub fn issue(r: &Read) -> Issue {
    Issue { item: item(r), events: events(r), linked: vec![] }
}

/// What a pinned PR's red checks offer: their page (the run's rerun is
/// GitHub's, and the mirror doesn't keep which run a check is).
pub fn rerun(pr: &Pr) -> Option<Rerun> {
    if pr.rollup != Some(model::CheckState::Failure) {
        return None;
    }
    let url = pr.checks.iter().find(|c| c.state.red() && !c.allow_failure).and_then(|c| c.url.clone());
    Some(Rerun {
        api: false,
        url,
        note: "This block reads through Arugula control: rerun them on the run's page".into(),
        runs: None,
        pipeline: None,
    })
}

#[cfg(test)]
mod tests {
    use arugula_control_wire::projects::ForgeText;

    use super::*;

    fn read() -> Read {
        Read {
            item: ForgeItem {
                id: "fi_0a".into(),
                repo: "acme/app".into(),
                number: 7,
                kind: "pr".into(),
                state: "open".into(),
                title: "a fix".into(),
                labels: vec!["bug".into()],
                author: Some("alice-gh".into()),
                assignees: vec![],
                reviewers: vec!["bob-gh".into(), "acme/core".into()],
                draft: false,
                merge_state: Some("blocked".into()),
                head_ref: Some("fix".into()),
                head_sha: Some("abc".into()),
                base_ref: Some("main".into()),
                created_at: 1,
                updated_at: 9,
            },
            checks: vec![
                ForgeCheck {
                    kind: "check_run".into(),
                    name: "build".into(),
                    status: "completed".into(),
                    conclusion: Some("failure".into()),
                    url: Some("https://github.com/acme/app/runs/1".into()),
                    updated_at: 5,
                },
                ForgeCheck {
                    kind: "status".into(),
                    name: "ci/lint".into(),
                    status: "completed".into(),
                    conclusion: Some("success".into()),
                    url: None,
                    updated_at: 5,
                },
            ],
            events: vec![
                ForgeEvent {
                    id: "1".into(),
                    kind: "opened".into(),
                    actor: Some("alice-gh".into()),
                    review_state: None,
                    at: 2,
                },
                ForgeEvent {
                    id: "2".into(),
                    kind: "review".into(),
                    actor: Some("bob-gh".into()),
                    review_state: Some("changes_requested".into()),
                    at: 3,
                },
                ForgeEvent {
                    id: "3".into(),
                    kind: "comment".into(),
                    actor: Some("bob-gh".into()),
                    review_state: None,
                    at: 4,
                },
            ],
            content: None,
        }
    }

    fn text(kind: &str, id: u64, author: &str, body: &str, at: u64) -> ForgeText {
        ForgeText {
            kind: kind.into(),
            id,
            author: Some(author.into()),
            body: body.into(),
            path: None,
            line: None,
            reply_to: None,
            created_at: at,
            updated_at: at,
        }
    }

    /// The mirror's rows as the reader's model: the same fields the block
    /// draws from GitHub, and the attention its rules find.
    #[test]
    fn the_mirror_as_the_model() {
        let p = pr(&read());
        let it = &p.item;
        assert_eq!(
            (it.url.as_str(), it.title.as_str(), it.author.as_str()),
            ("https://github.com/acme/app/pull/7", "a fix", "alice-gh")
        );
        assert_eq!(
            (it.head_ref.as_str(), it.head.branch.as_str(), it.head.sha.as_str(), it.base.branch.as_str()),
            ("refs/pull/7/head", "fix", "abc", "main")
        );
        assert_eq!((it.mergeable, it.blocked, it.comments), (Some(true), true, 1));
        assert_eq!(it.requested, vec![Reviewer::User("bob-gh".into()), Reviewer::Team("acme/core".into())]);
        assert_eq!(p.checks.len(), 2);
        assert_eq!(p.rollup, Some(model::CheckState::Failure));
        assert_eq!(p.reviews.len(), 1);
        assert_eq!(
            (p.reviews[0].author.as_deref(), p.reviews[0].state),
            (Some("bob-gh"), ReviewState::ChangesRequested)
        );
        assert_eq!(
            p.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
            [EventKind::Other, EventKind::Reviewed, EventKind::Commented]
        );
        assert_eq!(p.events[0].what.as_deref(), Some("opened it"));
        assert!(p.events.iter().all(|e| e.body.is_none()), "no text without /content");
        assert!(p.item.body.is_empty() && p.reviews[0].body.is_none());

        let me = model::Me { login: "alice-gh".into(), teams: vec![] };
        let wants = model::attention(&p, &me, 0);
        assert!(matches!(&wants[0], model::Want::Failed { why, .. } if why == "1 check failed: build"), "{wants:?}");
        assert!(matches!(&wants[1], model::Want::Changes { why } if why == "changes requested by bob-gh"));
        let bob = model::Me { login: "bob-gh".into(), teams: vec![] };
        assert!(matches!(&model::attention(&p, &bob, 0)[0], model::Want::Review { .. }));
        let r = rerun(&p).unwrap();
        assert_eq!((r.api, r.url.as_deref()), (false, Some("https://github.com/acme/app/runs/1")));
    }

    /// `/content` gives the description, the comments (in place of the
    /// mirror's rows that one was made) and the reviews' text.
    #[test]
    fn the_text_as_the_model() {
        let mut r = read();
        r.content = Some(ForgeContent {
            item: "fi_0a".into(),
            from: "github".into(),
            body: Some(text("body", 7, "alice-gh", "Fixes the thing.", 1)),
            comments: vec![
                text("review", 20, "Bob-GH", "needs a test", 3),
                text("comment", 21, "bob-gh", "ping @alice-gh", 4),
                text("review_comment", 22, "bob-gh", "nit", 5),
            ],
        });
        let p = pr(&r);
        assert_eq!(p.item.body, "Fixes the thing.");
        assert_eq!(p.item.comments, 2);
        assert_eq!(p.reviews[0].body.as_deref(), Some("needs a test"));
        assert_eq!(
            p.events.iter().map(|e| (e.kind, e.body.as_deref())).collect::<Vec<_>>(),
            [
                (EventKind::Other, None),
                (EventKind::Reviewed, Some("needs a test")),
                (EventKind::Commented, Some("ping @alice-gh")),
                (EventKind::ReviewComment, Some("nit")),
            ]
        );
        assert_eq!(p.events[2].id, "gh-21");
        let me = model::Me { login: "alice-gh".into(), teams: vec![] };
        assert!(model::attention(&p, &me, 0).iter().any(|w| matches!(w, model::Want::Mention { .. })));
        let text = model::ItemText::text(&p, "acme/app");
        assert!(text.contains("Fixes the thing.") && text.contains("ping @alice-gh"), "{text}");

        let i = issue(&r);
        assert_eq!((i.item.body.as_str(), i.item.comments), ("Fixes the thing.", 2));
    }

    #[test]
    fn content_is_asked_again_when_something_is_new() {
        let seen = (3, Some("3".to_owned()));
        let mut k = Kept { id: "fi_0a".into(), seen: seen.clone(), content: None, at: Instant::now() };
        assert!(!k.stale("fi_0a", &seen), "a failure waits");
        assert!(k.stale("fi_0b", &seen), "another item");
        assert!(k.stale("fi_0a", &(4, Some("4".into()))), "a new event");
        assert!(k.stale("fi_0a", &(3, Some("9".into()))), "a different newest one");
        k.at = Instant::now() - LOOK_AGAIN;
        assert!(k.stale("fi_0a", &seen), "a failure is tried again in time");
        k.content = Some(ForgeContent { item: "fi_0a".into(), from: "github".into(), body: None, comments: vec![] });
        assert!(!k.stale("fi_0a", &seen), "what it said stays");
    }

    #[test]
    fn merge_states_and_issues() {
        let mut r = read();
        for (s, m) in [(Some("dirty"), Some(false)), (Some("unknown"), None), (None, None), (Some("clean"), Some(true))]
        {
            r.item.merge_state = s.map(Into::into);
            assert_eq!(pr(&r).item.mergeable, m, "{s:?}");
        }
        r.item.kind = "issue".into();
        r.item.state = "closed".into();
        let i = issue(&r);
        assert_eq!(
            (i.item.kind, i.item.state, i.item.url.as_str()),
            (ItemKind::Issue, ItemState::Closed, "https://github.com/acme/app/issues/7")
        );
        assert!(i.item.head_ref.is_empty() && i.linked.is_empty());
    }

    #[test]
    fn only_the_platforms_ids() {
        assert!(good_id("fi_0123abcdef"));
        for bad in ["", "fi_", "fi_../x", "dft_12", "fi_12/checks", "FI_12"] {
            assert!(!good_id(bad), "{bad}");
        }
    }
}
