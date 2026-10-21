//! M170 (#1216): drafts on a linked repo are the platform's rows, so a
//! person sees and acts on them from the phone while this machine sleeps
//! ("The daemon's forge reader stays for unlinked repos; linked repos'
//! blocks are pins", aru-087).
//!
//! A GitHub block on github.com asks the platform, as this machine for its
//! owner (`GET /api/forge/repos/{owner}/{name}`), whether the repo is
//! linked to a project its owner may read it in. The answer is kept a few
//! minutes ([`link`]). When it is:
//! - **Waiting drafts in `layout.json` go up once,** each with an import
//!   key of its own (`<repo>#<number>/<draft id>`, which the platform makes
//!   this machine's). Only once the platform answers is the local copy
//!   dropped (and noted in the block's config); a crash before that uploads
//!   it again, and the platform answers with the row it made the first
//!   time. A draft the platform won't take (a merge, a rerun) stays here.
//! - **An agent's new draft goes to the platform** instead of the block.
//! - **The block shows the platform's drafts** for its item, read again
//!   every little while.
//!
//! When the platform can't be reached, or refuses, drafts stay in
//! `layout.json` and are tried again later: none is ever dropped before the
//! platform has it. Unlinked repos, Forgejo, GitLab, a machine outside
//! control and a control from before M170 keep drafts as they always were.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use arugula_control_wire::projects::{DraftCreated, ForgeDraft, ForgeDrafts, ForgeRepo, NewDraft};
use arugula_proto::forge::{Draft, DraftStatus, ReviewEvent, Write};
use reqwest::Method;

use crate::control::CloudRefused;

/// A repo linked for this machine's owner: its project, GitHub's id, and
/// the owner's GitHub login (M172; none from a control before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub project: String,
    pub repo_id: u64,
    pub login: Option<String>,
}

/// How long the platform's word on a repo is used.
const KEEP: Duration = Duration::from_secs(300);
/// How long after a failure to ask before asking again.
const RETRY: Duration = Duration::from_secs(60);

type Known = (Instant, Result<Option<Link>, String>);

static LINKS: LazyLock<Mutex<HashMap<String, Known>>> = LazyLock::new(Mutex::default);

/// Whether `repo`'s drafts are the platform's: the link, `None` when they
/// stay here (not linked, not readable by the owner, no control, or a
/// control from before M170), or why the platform couldn't say, to ask
/// again later.
pub async fn link(repo: &str) -> Result<Option<Link>, String> {
    let k = repo.to_ascii_lowercase();
    if let Some((until, known)) = LINKS.lock().unwrap().get(&k).cloned()
        && until > Instant::now()
    {
        return known;
    }
    let got = ask_link(repo).await;
    let keep = if got.is_ok() { KEEP } else { RETRY };
    LINKS.lock().unwrap().insert(k, (Instant::now() + keep, got.clone()));
    got
}

/// Forget the platform's word on a repo: it said otherwise since.
pub fn forget(repo: &str) {
    LINKS.lock().unwrap().remove(&repo.to_ascii_lowercase());
}

async fn ask_link(repo: &str) -> Result<Option<Link>, String> {
    let Some(control) = super::live::control() else { return Ok(None) };
    if repo.split('/').count() != 2 {
        return Ok(None);
    }
    match control.cloud_now(Method::GET, &format!("/api/forge/repos/{repo}"), None).await {
        Ok(v) => {
            let r: ForgeRepo = serde_json::from_value(v).map_err(|e| format!("the platform's answer: {e}"))?;
            Ok(match (r.linked, r.project, r.repo_id) {
                (true, Some(project), Some(repo_id)) => Some(Link { project, repo_id, login: r.login }),
                _ => None,
            })
        }
        // Not in control, or a control without the route: drafts stay.
        Err(CloudRefused::NotJoined | CloudRefused::OldControl | CloudRefused::Said { status: 404, .. }) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Why the platform didn't take a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// It couldn't be reached, or was busy: try again later.
    Later(String),
    /// It refused this one (what it says, for the log): it stays here.
    Refused(String),
    /// The repo isn't linked (for the owner) any more.
    NotLinked,
}

pub(super) fn failed(e: CloudRefused) -> Failed {
    match e {
        CloudRefused::Said { status: 404, .. } => Failed::NotLinked,
        CloudRefused::Said { status: 400 | 409, said, .. } => Failed::Refused(said),
        e => Failed::Later(e.to_string()),
    }
}

/// A local draft's own id on the platform: the block's draft id under its
/// item, which the platform makes this machine's.
pub fn import_key(repo: &str, number: u64, id: &str) -> String {
    format!("{}#{number}/{id}", repo.to_ascii_lowercase())
}

fn github_event(e: ReviewEvent) -> &'static str {
    match e {
        ReviewEvent::Approve => "APPROVE",
        ReviewEvent::RequestChanges => "REQUEST_CHANGES",
        ReviewEvent::Comment => "COMMENT",
    }
}

/// A local draft as the platform takes it: comments and reviews. `None`
/// for a merge, a rerun or live updates, which stay on the block.
pub fn new_draft(repo: &str, number: u64, d: &Draft) -> Option<NewDraft> {
    let (kind, event, body) = match &d.write {
        Write::Comment { body } => ("comment", None, body.clone()),
        Write::Review { event, body } => ("review", Some(github_event(*event)), body.clone().unwrap_or_default()),
        Write::Merge { .. } | Write::Rerun | Write::Live { .. } => return None,
    };
    Some(NewDraft {
        repo: repo.to_owned(),
        number: Some(number),
        kind: kind.into(),
        event: event.map(Into::into),
        title: None,
        body,
        agent: Some(d.by.clone()),
        import_key: Some(import_key(repo, number, &d.id)),
    })
}

/// Make a draft on the platform (or find the one an earlier upload of the
/// same key made).
pub async fn create(new: &NewDraft) -> Result<ForgeDraft, Failed> {
    let control = super::live::control().ok_or(Failed::NotLinked)?;
    let body = serde_json::to_value(new).map_err(|e| Failed::Refused(e.to_string()))?;
    let v = control.cloud_now(Method::POST, "/api/forge/drafts", Some(&body)).await.map_err(failed)?;
    let got: DraftCreated =
        serde_json::from_value(v).map_err(|e| Failed::Later(format!("the platform's answer: {e}")))?;
    Ok(got.draft)
}

/// The platform's waiting drafts on an item.
pub async fn list(link: &Link, repo: &str, number: u64) -> Result<Vec<ForgeDraft>, Failed> {
    let control = super::live::control().ok_or(Failed::NotLinked)?;
    let path = format!("/api/projects/{}/drafts?repo={repo}&number={number}", link.project);
    let v = control.cloud_now(Method::GET, &path, None).await.map_err(failed)?;
    let got: ForgeDrafts =
        serde_json::from_value(v).map_err(|e| Failed::Later(format!("the platform's answer: {e}")))?;
    Ok(got.drafts)
}

/// Discard a platform draft: a person dropped it here.
pub async fn discard(id: &str) -> Result<(), String> {
    let control = super::live::control().ok_or("not in Arugula control any more")?;
    control
        .cloud_now(Method::POST, &format!("/api/forge/drafts/{id}/discard"), None)
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

/// A platform draft as the block shows it, `by` the agent that wrote it,
/// or `author` (a name for its author). `None` for what a block doesn't
/// show (a new issue).
pub fn shown(d: &ForgeDraft, author: &str, error: Option<String>) -> Option<Draft> {
    let body = d.body.clone();
    let write = match (d.kind.as_str(), d.event.as_deref()) {
        ("comment", _) => Write::Comment { body },
        ("review", e) => Write::Review {
            event: match e {
                Some("APPROVE") => ReviewEvent::Approve,
                Some("REQUEST_CHANGES") => ReviewEvent::RequestChanges,
                _ => ReviewEvent::Comment,
            },
            body: Some(body).filter(|b| !b.is_empty()),
        },
        _ => return None,
    };
    Some(Draft {
        id: d.id.clone(),
        write,
        by: d.agent.clone().unwrap_or_else(|| author.to_owned()),
        at_ms: d.created_at,
        status: DraftStatus::Waiting,
        settled_by: None,
        settled_ms: None,
        url: None,
        error,
    })
}

/// Whether a draft id is the platform's (`dft_`), not the block's (`d…`).
pub fn is_platform_id(id: &str) -> bool {
    id.starts_with("dft_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(id: &str, write: Write) -> Draft {
        Draft {
            id: id.into(),
            write,
            by: "mcp:claude-code".into(),
            at_ms: 5,
            status: DraftStatus::Waiting,
            settled_by: None,
            settled_ms: None,
            url: None,
            error: None,
        }
    }

    /// Comments and reviews go up, in GitHub's words, each with a key that
    /// stays the same however often it's tried; the rest stays here.
    #[test]
    fn what_goes_up_and_its_key() {
        let c = new_draft("Acme/App", 7, &draft("d17", Write::Comment { body: "lgtm".into() })).unwrap();
        assert_eq!(
            (c.kind.as_str(), c.number, c.body.as_str(), c.agent.as_deref()),
            ("comment", Some(7), "lgtm", Some("mcp:claude-code"))
        );
        assert_eq!(c.import_key.as_deref(), Some("acme/app#7/d17"));
        let again = new_draft("acme/app", 7, &draft("d17", Write::Comment { body: "lgtm".into() })).unwrap();
        assert_eq!(again.import_key, c.import_key, "the same key on a second try");
        let r =
            new_draft("acme/app", 7, &draft("d18", Write::Review { event: ReviewEvent::Approve, body: None })).unwrap();
        assert_eq!((r.kind.as_str(), r.event.as_deref(), r.body.as_str()), ("review", Some("APPROVE"), ""));
        for w in [Write::Merge { style: None }, Write::Rerun, Write::Live { on: true }] {
            assert!(new_draft("acme/app", 7, &draft("d19", w)).is_none());
        }
    }

    /// The platform's draft as the block shows it.
    #[test]
    fn a_platform_draft_on_the_block() {
        let mut d = ForgeDraft {
            id: "dft_1".into(),
            kind: "review".into(),
            event: Some("REQUEST_CHANGES".into()),
            body: "split it".into(),
            agent: Some("mcp:codex".into()),
            created_at: 9,
            ..ForgeDraft::default()
        };
        let s = shown(&d, "Jake", None).unwrap();
        assert_eq!(s.write, Write::Review { event: ReviewEvent::RequestChanges, body: Some("split it".into()) });
        assert_eq!((s.by.as_str(), s.at_ms, s.status), ("mcp:codex", 9, DraftStatus::Waiting));
        d.agent = None;
        d.kind = "comment".into();
        assert_eq!(
            shown(&d, "Jake", Some("no".into())).map(|s| (s.by, s.error)),
            Some(("Jake".into(), Some("no".into())))
        );
        d.kind = "issue".into();
        assert!(shown(&d, "Jake", None).is_none());
        assert!(is_platform_id("dft_1") && !is_platform_id("d1700000000000"));
    }

    #[test]
    fn refusals() {
        let said = |status| CloudRefused::Said { status, said: "no".into(), reason: None };
        assert_eq!(failed(said(404)), Failed::NotLinked);
        assert_eq!(failed(said(400)), Failed::Refused("no".into()));
        assert!(matches!(failed(said(429)), Failed::Later(_)));
        assert!(matches!(failed(CloudRefused::Unreachable("down".into())), Failed::Later(_)));
        assert!(matches!(failed(CloudRefused::Maintenance), Failed::Later(_)));
    }
}
