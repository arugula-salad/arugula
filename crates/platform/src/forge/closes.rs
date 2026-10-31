//! A merged PR closes the native issues it says it closes (Arugula #432):
//! `Closes Arugula #12`, `fixes: arugula #12` or `Resolves iss_…` in its
//! title or body. A bare `Arugula #12` only links ([`crate::boards::refs`]),
//! and a bare `Closes #12` is a GitHub issue's, which GitHub closes itself.
//!
//! It runs wherever the mirror learns of a PR: a webhook ([`webhook`]) and
//! the poller's turns ([`merged`]). Only an issue of the project the PR's
//! repo is linked to is closed, as completed, and only an open one: an issue
//! closed already (for any reason) stays as it is, so a redelivery or a
//! second pass over the same PR adds nothing. It is closed as the person
//! who merged the PR when they are in the project, else as the App; the
//! event names the PR.

use arugula_e2e::now_ms;
use serde_json::Value;
use tracing::warn;

use crate::{
    Platform,
    db::{
        forge::MergedPr,
        issues::{Actor, CloseReason, Issue, Patch, State},
    },
};

/// Who an issue is closed as when no member of its project merged the PR:
/// the App, named as GitHub's text is in a thread (`github:<login>`).
pub const APP: &str = "github:arugula";

/// A native issue as a PR names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    /// `Arugula #N`.
    Number(u64),
    /// `iss_…`.
    Id(String),
}

const CLOSING: [&str; 9] = ["closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve"];

/// The native issues `text` closes: a closing word (any case), a colon if
/// it likes, then `Arugula #N` or an `iss_` id. Each once, in order.
pub fn closing(text: &str) -> Vec<Named> {
    let mut out: Vec<Named> = vec![];
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_alphabetic() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let end = (i..b.len()).find(|&j| !b[j].is_ascii_alphabetic()).unwrap_or(b.len());
        if CLOSING.contains(&text[i..end].to_ascii_lowercase().as_str())
            && let Some(n) = named(&text[end..])
            && !out.contains(&n)
        {
            out.push(n);
        }
        i = end;
    }
    out
}

/// The issue that starts `s` after a closing word: a colon or a space (or
/// both), then `Arugula #N` or `iss_…`.
fn named(s: &str) -> Option<Named> {
    let rest = s.strip_prefix(':').unwrap_or(s);
    let spaced = rest.trim_start_matches([' ', '\t']);
    if rest.len() == s.len() && spaced.len() == rest.len() {
        return None;
    }
    if let Some(word) = spaced.get(..7).filter(|w| w.eq_ignore_ascii_case("arugula")) {
        let after = spaced[word.len()..].trim_start_matches([' ', '\t']);
        if after.len() == spaced.len() - word.len() {
            return None;
        }
        let digits = after.strip_prefix('#')?;
        let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
        if end == 0 || end > 9 || digits[end..].starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return None;
        }
        return Some(Named::Number(digits[..end].parse().ok()?));
    }
    let id = spaced.strip_prefix("iss_")?;
    let end = id.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(id.len());
    (end > 0).then(|| Named::Id(format!("iss_{}", &id[..end])))
}

/// A `pull_request` webhook, after the mirror wrote it: the issues the PR
/// closes if it is merged now. Its body is read from the content the same
/// delivery wrote, so this runs after [`super::ingest::webhook`].
pub async fn webhook(p: &Platform, event: &str, v: &Value) {
    if event != "pull_request" {
        return;
    }
    if let (Some(repo), Some(number)) = (v["repository"]["id"].as_u64(), v["pull_request"]["number"].as_u64()) {
        merged(p, repo, &[number]).await;
    }
}

/// The PRs of `repo` among these `numbers` that are merged: each closes the
/// open native issues of the repo's project that it names.
pub async fn merged(p: &Platform, repo: u64, numbers: &[u64]) {
    let link = match p.db.linked_repo(repo) {
        Ok(Some(l)) if l.unlinked_at.is_none() => l,
        Ok(_) => return,
        Err(e) => return warn!(error = %e, "closing issues: finding the project of a repo"),
    };
    let prs = match p.db.merged_prs(repo, numbers) {
        Ok(prs) => prs,
        Err(e) => return warn!(error = %e, "closing issues: reading merged PRs"),
    };
    for pr in prs {
        if let Err(e) = close_for(p, &link.project_id, &link.full_name, &pr).await {
            warn!(repo, number = pr.number, error = format!("{e:#}"), "closing the issues a merged PR names");
        }
    }
}

async fn close_for(p: &Platform, project: &str, repo: &str, pr: &MergedPr) -> anyhow::Result<()> {
    let mut named = closing(&pr.title);
    for n in closing(pr.body.as_deref().unwrap_or("")) {
        if !named.contains(&n) {
            named.push(n);
        }
    }
    let mut issues: Vec<Issue> = vec![];
    for n in &named {
        let found = match n {
            Named::Number(n) => p.db.issue_by_number(project, *n)?,
            Named::Id(id) => p.db.issue(id)?.filter(|i| i.project == project),
        };
        if let Some(i) = found.filter(|i| i.state == State::Open && !issues.iter().any(|x| x.id == i.id)) {
            issues.push(i);
        }
    }
    if issues.is_empty() {
        return Ok(());
    }
    let merger = merger(p, project, pr.merged_by).await;
    let by = Actor::person(merger.as_deref().unwrap_or(APP));
    let reference = format!("{repo}#{}", pr.number);
    for i in issues {
        let patch = Patch { state: Some(State::Closed(CloseReason::Completed)), by_pr: Some(&reference), ..Patch::default() };
        let Some((issue, events)) = p.db.patch_issue(&i.id, &patch, by, now_ms())? else { continue };
        if !events.is_empty() {
            // As `issues::change` tells those who show it.
            p.chat.object_changed(&p.db, &format!("project:{}", issue.project), &issue.id);
            crate::boards::issue_changed(p, &issue.id);
        }
    }
    Ok(())
}

/// The account of whoever merged (by GitHub's id), if they are in `project`.
async fn merger(p: &Platform, project: &str, github: Option<u64>) -> Option<String> {
    let account = p.hub.github_account(github?).await.ok().flatten()?;
    let project = p.db.issue_project(project).ok().flatten()?;
    let member = match (&project.account, &project.team) {
        (Some(a), _) => *a == account,
        (None, Some(t)) => p.cache.team(&*p.hub, t).await.ok().flatten().is_some_and(|t| t.role(&account).is_some()),
        _ => false,
    };
    member.then_some(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(n: u64) -> Named {
        Named::Number(n)
    }

    /// The forms that close an issue, and those that only mention one.
    #[test]
    fn a_closing_word_then_arugula_or_an_id() {
        assert_eq!(closing("Closes Arugula #12"), [n(12)]);
        assert_eq!(closing("fixes: arugula #7. RESOLVED:ARUGULA  #8"), [n(7), n(8)]);
        assert_eq!(closing("Fix ARUGULA #3 and resolved Arugula #4, Closes Arugula #3"), [n(3), n(4)]);
        let id = "iss_f587865d2dc6ad3e39c4957fcb227b5c";
        assert_eq!(closing(&format!("Resolves {id}.")), [Named::Id(id.into())]);
        assert_eq!(closing(&format!("resolves {id}, closed ARUGULA #4")), [Named::Id(id.into()), n(4)]);
        assert_eq!(closing("closes: iss_ab12"), [Named::Id("iss_ab12".into())]);
    }

    #[test]
    fn what_only_links_or_isnt_ours_closes_nothing() {
        for text in [
            "Arugula #12",
            "Closes #12",
            "Closes the Arugula #12 bug",
            "Closes Arugula 12",
            "Closes Arugula#12",
            "Closes Arugula #12x",
            "Closes Arugula #",
            "Closes Arugula #1234567890",
            "Disclosures Arugula #12",
            "Closesy Arugula #12",
            "Closes iss_",
            "Closes",
            "",
        ] {
            assert_eq!(closing(text), Vec::<Named>::new(), "{text:?}");
        }
        assert_eq!(closing("Closes:Arugula #5"), [n(5)], "a colon is a separator enough");
    }
}
