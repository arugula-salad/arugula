//! Open on GitHub, per issue (M205, Arugula #62): "Taking custody of issues
//! stays reversible" (aru-089) and "An issue opened on GitHub stays native,
//! and only new comments cross" (aru-130).
//!
//! - **It's a draft** ([`draft`]): a native issue (`drafts.native_issue`)
//!   as a new issue on a repo linked to its project, the project's one
//!   linked repo unless another is named. An owner or editor of the issue
//!   makes it, or their agent with its machine's `issues.write` grant (and
//!   `cloud.drive`, as any draft): an agent's waits for its person
//!   (aru-041). One export at a time per issue, and none once it's on
//!   GitHub. Its title and body are the issue's, read again as it's sent.
//! - **A person sends it** through aru-085's path ([`super::send`]): a
//!   one-use user token, else one of their machines. Sent ([`sent`]), the
//!   issue's GitHub origin is set (`issues.github_*`) and its timeline gets
//!   `exported`, as the person who sent it. The mirror then has the GitHub
//!   issue, and work items take the two as one (`db::work_items`).
//! - **The native issue stays** as it was: open or closed, with its number,
//!   timeline and thread, and a link to where it now lives. Nothing else of
//!   it is kept in step with GitHub (title, body, state, labels and
//!   assignees may drift): there's no reconciler.
//! - **Comments cross, for exported issues only:**
//!   - **in** ([`webhook`], [`polled`]): a comment made on the GitHub issue
//!     lands in the native thread as GitHub's (no author, `via`
//!     `github:<login>`), through the chat crate
//!     ([`crate::features::Chat::github_comment`]). One whose webhook never
//!     came is found by the poller's next turn over the issue. Only from a
//!     public repo: the thread is the whole project's, and a private repo's
//!     text reaches only those GitHub lets read it ("Private items are
//!     shown per viewer, by their own GitHub access", aru-086);
//!   - **out** ([`posted`]): a person's comment in the thread (not a reply
//!     kept in a sub-thread) is a draft (`native_seq`) sent as them at once
//!     through one of their machines; with none awake it waits in their
//!     drafts, and the post says so. An agent's is a draft its person
//!     sends, and stays native until then;
//!   - **never back:** each GitHub comment that crossed is a row of
//!     `issue_github_comments`, by GitHub's id, either way. GitHub's copy of
//!     a comment that went out (its webhook may come before the send's
//!     answer) is matched to its draft by its text and kept out, and what
//!     came in is posted as GitHub's, which [`posted`] never sends.
//!   - Edits and deletions don't cross, and comments from before the
//!     export stay native.
//!
//! **Never logged:** a title, body or comment's text; only ids, numbers and
//! accounts.

use arugula_control_common::{ApiError, err};
use arugula_control_wire::{chat::ChatToGithub, projects as wire};
use arugula_e2e::now_ms;
use arugula_forge_github::Github;
use axum::http::StatusCode;
use serde_json::Value;
use tracing::{debug, info, warn};

use super::{
    Purpose,
    drafts::{DRAFTS_MADE, Drafter, wire_drafts},
    poller::RepoTokens,
    send::{self, How},
    viewer::{self, Viewer},
};
use crate::{
    Platform,
    db::{
        forge::{DraftRow, LinkedRepo, NewDraftRow, SENDING_MS},
        issues::{Actor, Crossed, GithubOrigin, Issue},
    },
};

/// How long after a comment went out a GitHub comment with its text is
/// taken for its copy, when the send's answer named no comment.
pub const ECHO_MS: u64 = 24 * 3_600_000;

/// What an agent's comment on an exported issue does.
pub const AGENT_WAITS: &str = "an agent's comment waits as a draft for its person to send to GitHub";

fn bad(why: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, why)
}

fn conflict(why: &str) -> ApiError {
    err(StatusCode::CONFLICT, why)
}

/// GitHub's number in an issue's URL (`…/issues/12`).
pub fn issue_number(url: &str) -> Option<u64> {
    let path = url.split(['#', '?']).next()?.trim_end_matches('/');
    let (rest, n) = path.rsplit_once('/')?;
    rest.ends_with("/issues").then(|| n.parse().ok()).flatten()
}

/// GitHub's id of a comment in its URL (`…#issuecomment-34`).
pub fn comment_id(url: &str) -> Option<u64> {
    url.rsplit_once("#issuecomment-")?.1.parse().ok()
}

/// The repo an issue goes to: `asked`, else its project's one linked repo.
fn target(app: &Platform, issue: &Issue, asked: &str) -> Result<LinkedRepo, ApiError> {
    let live: Vec<LinkedRepo> =
        app.db.linked_repos(&issue.project)?.into_iter().filter(|l| l.unlinked_at.is_none()).collect();
    let asked = asked.trim();
    if asked.is_empty() {
        return match live.len() {
            0 => Err(conflict("its project has no linked repo: link one, then open it on GitHub")),
            1 => Ok(live.into_iter().next().expect("one")),
            _ => {
                let names: Vec<&str> = live.iter().map(|l| l.full_name.as_str()).collect();
                Err(bad(&format!("its project links {}: say which repo", names.join(", "))))
            }
        };
    }
    if !super::good_repo(asked) {
        return Err(bad("repo: OWNER/NAME"));
    }
    live.into_iter()
        .find(|l| l.full_name.eq_ignore_ascii_case(asked))
        .ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("{asked} isn't linked to this issue's project")))
}

/// `POST /api/forge/drafts` with `native_issue` (module docs): the draft
/// that opens the issue on GitHub, or the one that does already.
pub(super) async fn draft(
    app: &Platform,
    d: &Drafter,
    b: &wire::NewDraft,
    native: &str,
) -> Result<wire::DraftCreated, ApiError> {
    if b.kind != "issue" || b.number.is_some() {
        return Err(bad("a native issue goes to GitHub as a new issue: kind issue, and no number"));
    }
    let agent = b.agent.as_deref().map(str::trim).filter(|a| !a.is_empty());
    let agent = match (&d.machine, agent) {
        (_, Some(a)) if a.len() > 100 => return Err(bad("agent: a short name")),
        (Some(_), None) => Some("an agent"),
        (_, a) => a,
    };
    // Read first, so someone who may read it learns it's there; then as a
    // write: an owner's or editor's, and an agent's `issues.write`.
    let by = Actor { account: &d.account, agent: d.machine.as_deref() };
    let (issue, _) = crate::issues::issue_place(app, by, native, false).await?;
    crate::issues::place(app, by, &issue.project, true).await?;
    if let Some(g) = &issue.github {
        return Err(conflict(&format!("it's on GitHub already, as {}#{}", g.repo, g.number)));
    }
    let link = target(app, &issue, &b.repo)?;
    let v = Viewer::of(app, &d.account).await?;
    let checked = viewer::check(app, &v, std::slice::from_ref(&link)).await?;
    if !checked.readable.has(link.repo_id) {
        return Err(err(StatusCode::FORBIDDEN, &format!("GitHub doesn't let you read {}", link.full_name)));
    }
    let now = now_ms();
    for had in app.db.export_drafts(&issue.id)? {
        if had.author != d.account {
            return Err(conflict(&format!(
                "another person's draft ({}) opens it on GitHub already: they send it, or it's discarded first",
                had.id
            )));
        }
        if had.sending(now) {
            return Err(conflict("it's being sent"));
        }
        if had.repo_id == link.repo_id {
            let draft = wire_drafts(app, vec![had]).await?.remove(0);
            return Ok(wire::DraftCreated { draft, created: false });
        }
        // Another repo now: that one goes.
        app.db.discard_draft(crate::db::forge::Discarder::Author(&d.account), &had.id, now)?;
    }
    app.limits.check_account(DRAFTS_MADE, &d.account)?;
    let new = NewDraftRow {
        project_id: &link.project_id,
        author: &d.account,
        agent_name: agent,
        agent_machine: d.machine.as_deref(),
        repo_id: link.repo_id,
        number: None,
        kind: "issue",
        event: None,
        title: Some(issue.title.trim()),
        body: &issue.body,
        import_key: None,
        native_issue: Some(&issue.id),
        native_seq: None,
        now,
    };
    let (row, created) = app.db.add_draft(&checked.readable, &new)?.ok_or_else(|| conflict("its repo was unlinked"))?;
    info!(
        draft = row.id,
        account = d.account,
        machine = d.machine,
        issue = issue.id,
        repo = row.repo_id,
        "a native issue's export drafted"
    );
    let draft = wire_drafts(app, vec![row]).await?.remove(0);
    Ok(wire::DraftCreated { draft, created })
}

/// Before a send of `d`: an issue's export goes only while the issue isn't
/// on GitHub, and with its title and body as they are now.
pub(super) fn before_send(app: &Platform, d: &DraftRow) -> Result<(), ApiError> {
    let (Some(native), "issue") = (d.native_issue.as_deref(), d.kind.as_str()) else { return Ok(()) };
    let issue = app.db.issue(native)?.ok_or_else(|| conflict("its native issue is gone"))?;
    if let Some(g) = &issue.github {
        return Err(conflict(&format!(
            "its issue is on GitHub already, as {}#{}: discard this draft",
            g.repo, g.number
        )));
    }
    app.db.refresh_export_draft(&d.id, issue.title.trim(), &issue.body, now_ms())?;
    Ok(())
}

/// After a draft of a native issue's was sent (its `sent_url` GitHub's):
/// the issue's origin and `exported`, or its comment's link.
pub(super) async fn sent(app: &Platform, d: &DraftRow) {
    let Some(native) = d.native_issue.as_deref() else { return };
    let Some(url) = d.sent_url.as_deref() else {
        return warn!(draft = d.id, issue = native, "a native issue's draft sent, and GitHub named nothing to link");
    };
    let now = now_ms();
    match d.kind.as_str() {
        "issue" => {
            let Some(number) = issue_number(url) else {
                return warn!(draft = d.id, issue = native, "an export sent to a URL that names no issue");
            };
            let to = GithubOrigin { repo: d.full_name.clone(), repo_id: d.repo_id, number };
            match app.db.set_issue_github(native, &to, Actor::person(&d.author), now) {
                Ok(Some((i, _))) => {
                    info!(
                        issue = i.id,
                        repo = d.repo_id,
                        number,
                        account = d.author,
                        "a native issue opened on GitHub"
                    );
                    app.chat.object_changed(&app.db, &format!("project:{}", i.project), &i.id);
                    crate::boards::issue_changed(app, &i.id);
                }
                Ok(None) => warn!(issue = native, number, "an export sent for an issue on GitHub already"),
                Err(e) => warn!(issue = native, error = format!("{e:#}"), "setting an issue's GitHub origin"),
            }
        }
        "comment" => {
            let (Some(seq), Some(id)) = (d.native_seq, comment_id(url)) else { return };
            if let Err(e) = app.db.claim_github_comment(id, native, Some(seq), Crossed::Out, now) {
                warn!(issue = native, comment = id, error = format!("{e:#}"), "linking a comment sent to GitHub");
            }
        }
        _ => {}
    }
}

/// A comment GitHub has on an exported issue.
#[derive(Clone, PartialEq, Eq)]
pub struct Theirs {
    pub id: u64,
    pub login: String,
    pub body: String,
}

impl Theirs {
    /// From GitHub's comment, as a webhook or a list has it.
    pub fn of(v: &Value) -> Option<Self> {
        Some(Theirs {
            id: v["id"].as_u64()?,
            login: v["user"]["login"].as_str().unwrap_or("ghost").to_owned(),
            body: v["body"].as_str().unwrap_or_default().to_owned(),
        })
    }
}

/// Text as GitHub may give it back: its own line ends, and no blank ends.
fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace("\r\n", "\n").trim().to_owned();
    norm(a) == norm(b)
}

/// GitHub's `issue_comment` `created` on an exported issue's GitHub issue
/// (not a PR's): into the native thread.
pub async fn webhook(p: &Platform, event: &str, v: &Value) {
    if event != "issue_comment" || v["action"] != "created" || !v["issue"]["pull_request"].is_null() {
        return;
    }
    let (Some(repo), Some(number)) = (v["repository"]["id"].as_u64(), v["issue"]["number"].as_u64()) else { return };
    let Some(c) = Theirs::of(&v["comment"]) else { return };
    match p.db.exported_issue(repo, number) {
        Ok(Some(i)) => bring_in(p, &i, &[c]).await,
        Ok(None) => {}
        Err(e) => warn!(repo, number, error = format!("{e:#}"), "finding an exported issue"),
    }
}

/// The poller saw these issues of `link` change: an exported one's GitHub
/// comments that its thread hasn't got (a webhook that never came). Read
/// with the poller's own read token, within the installation's reserve.
pub async fn polled(p: &Platform, link: (u64, u64, &str), numbers: &[u64]) {
    let (repo, installation, name) = link;
    let Some(gh) = &p.github else { return };
    for &n in numbers {
        let issue = match p.db.exported_issue(repo, n) {
            Ok(Some(i)) => i,
            Ok(None) => continue,
            Err(e) => {
                warn!(repo, number = n, error = format!("{e:#}"), "finding an exported issue");
                continue;
            }
        };
        if gh.ingest.stats.reserved(installation, 0, now_ms() / 1000).is_some() {
            return;
        }
        let tokens = RepoTokens { p, caller: "platform", purpose: Purpose::Ingest, installation, repo_id: repo };
        let client = Github::new(&gh.api, p.http.clone(), tokens);
        let path = format!("repositories/{repo}/issues/{n}/comments?per_page=100");
        match client.page(name, &path).await {
            Ok((page, _)) => {
                let theirs: Vec<Theirs> = page.as_array().into_iter().flatten().filter_map(Theirs::of).collect();
                bring_in(p, &issue, &theirs).await;
            }
            Err(e) => warn!(repo, number = n, error = %e, "reading an exported issue's comments"),
        }
        let r = client.rate();
        gh.ingest.stats.heard(installation, r.limit, r.remaining, r.reset);
    }
}

/// GitHub's comments on `issue`'s GitHub issue into its thread: each one
/// once, and none that went out from here.
pub async fn bring_in(p: &Platform, issue: &Issue, comments: &[Theirs]) {
    let Some(g) = issue.github.as_ref().filter(|_| !comments.is_empty()) else { return };
    // While its repo is linked to the issue's project, and public: the
    // thread is the whole project's, and a private repo's text reaches only
    // those GitHub lets read it (aru-086).
    match p.db.linked_repo(g.repo_id) {
        Ok(Some(l)) if l.unlinked_at.is_none() && l.project_id == issue.project && !l.private => {}
        Ok(_) => {
            return debug!(
                issue = issue.id,
                repo = g.repo_id,
                "GitHub's comments stay there: a private or unlinked repo"
            );
        }
        Err(e) => return warn!(issue = issue.id, error = format!("{e:#}"), "an exported issue's repo"),
    }
    let now = now_ms();
    let (crossed, mut out) = match (
        p.db.github_comments(&issue.id),
        p.db.outgoing_comments(&issue.id, now.saturating_sub(SENDING_MS), now.saturating_sub(ECHO_MS)),
    ) {
        (Ok(c), Ok(o)) => (c, o),
        (Err(e), _) | (_, Err(e)) => {
            return warn!(issue = issue.id, error = format!("{e:#}"), "an exported issue's comments");
        }
    };
    for c in comments {
        if crossed.contains_key(&c.id) || c.body.trim().is_empty() {
            continue;
        }
        // One of ours, back from GitHub: its draft's, and kept out.
        if let Some(at) = out.iter().position(|o| same(&o.body, &c.body)) {
            let o = out.remove(at);
            if let Err(e) = p.db.claim_github_comment(c.id, &issue.id, Some(o.seq), Crossed::Out, now) {
                warn!(issue = issue.id, comment = c.id, error = format!("{e:#}"), "linking a comment sent to GitHub");
            }
            continue;
        }
        match p.db.claim_github_comment(c.id, &issue.id, None, Crossed::In, now) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                warn!(issue = issue.id, comment = c.id, error = format!("{e:#}"), "claiming a GitHub comment");
                continue;
            }
        }
        match crate::features::chat().github_comment(p, &issue.id, &c.login, &c.body).await {
            Ok(seq) => {
                if let Err(e) = p.db.github_comment_landed(c.id, seq) {
                    warn!(
                        issue = issue.id,
                        comment = c.id,
                        error = format!("{e:#}"),
                        "noting a GitHub comment's message"
                    );
                }
                info!(
                    issue = issue.id,
                    comment = c.id,
                    seq,
                    "a GitHub comment brought into an exported issue's thread"
                );
            }
            Err(e) => {
                warn!(issue = issue.id, comment = c.id, error = format!("{e:#}"), "bringing a GitHub comment in");
                let _ = p.db.unclaim_github_comment(c.id);
            }
        }
    }
}

/// A comment posted in a native issue's thread (`issue`, message `seq`) by
/// `author`, or their agent on `machine`: on an exported issue it goes to
/// GitHub (module docs), and this says how far. `None` when the issue
/// isn't on GitHub.
pub async fn posted(
    app: &Platform,
    issue: &str,
    author: &str,
    machine: Option<&str>,
    seq: i64,
    text: &str,
) -> Option<ChatToGithub> {
    let i = match app.db.issue(issue) {
        Ok(Some(i)) => i,
        Ok(None) => return None,
        Err(e) => {
            warn!(issue, error = format!("{e:#}"), "reading an issue a comment was posted on");
            return None;
        }
    };
    let g = i.github.as_ref()?;
    let stays = |why: String| Some(ChatToGithub { why: Some(why), ..Default::default() });
    let link = match app.db.linked_repo(g.repo_id) {
        Ok(Some(l)) if l.unlinked_at.is_none() && l.project_id == i.project => l,
        Ok(_) => return stays(format!("{} isn't linked to its project now: it stays here", g.repo)),
        Err(e) => return stays(format!("{e:#}")),
    };
    let made = async {
        let v = Viewer::of(app, author).await?;
        let checked = viewer::check(app, &v, std::slice::from_ref(&link)).await?;
        let new = NewDraftRow {
            project_id: &link.project_id,
            author,
            agent_name: machine.map(|_| "an agent"),
            agent_machine: machine,
            repo_id: link.repo_id,
            number: Some(g.number),
            kind: "comment",
            event: None,
            title: None,
            body: text,
            import_key: None,
            native_issue: Some(&i.id),
            native_seq: Some(seq),
            now: now_ms(),
        };
        let row = app.db.add_draft(&checked.readable, &new)?.map(|(r, _)| r);
        Ok::<_, ApiError>(row)
    };
    let row = match made.await {
        Ok(Some(r)) => r,
        Ok(None) => return stays(format!("GitHub doesn't let you read {}: it stays here", link.full_name)),
        Err(e) => return stays(e.1),
    };
    super::inbox::drafted(app, row.repo_id, row.number);
    if machine.is_some() {
        info!(draft = row.id, issue, seq, "an agent's comment on an exported issue drafted");
        return Some(ChatToGithub { draft: row.id, why: Some(AGENT_WAITS.into()), ..Default::default() });
    }
    match send::send(app, author, &row.id, How::Machine).await {
        Ok(s) => Some(ChatToGithub { draft: row.id, sent: true, url: s.draft.sent_url, why: None }),
        Err(e) => Some(ChatToGithub { draft: row.id, sent: false, url: None, why: Some(e.1) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_githubs_urls() {
        assert_eq!(issue_number("https://github.com/acme/app/issues/12"), Some(12));
        assert_eq!(issue_number("https://github.com/acme/app/issues/12/"), Some(12));
        assert_eq!(issue_number("https://github.com/acme/app/issues/12#issuecomment-5"), Some(12));
        assert_eq!(issue_number("https://github.com/acme/app/pull/12"), None);
        assert_eq!(issue_number("https://github.com/acme/app/issues"), None);
        assert_eq!(comment_id("https://github.com/acme/app/issues/12#issuecomment-34"), Some(34));
        assert_eq!(comment_id("https://github.com/acme/app/issues/12"), None);
        assert!(same("a\r\nb \n", "a\nb"));
        assert!(!same("a", "b"));
    }
}
