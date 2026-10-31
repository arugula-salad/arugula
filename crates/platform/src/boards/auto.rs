//! A PR attaches itself to the row of the issue it names (Arugula #368,
//! aru-125). When the mirror writes a PR (a webhook, the poller's
//! reconcile or its backfill), [`auto_attach`] reads what the PR references
//! ([`super::refs`]) and makes its automatic attachments say so: `work` on
//! each row of the project's boards whose issue it names, so the row moves
//! to Review, and to Done when the PR merges; and none on a row it stopped
//! naming. No one writes to the board.
//!
//! Only the boards of the project the PR's repo is linked to are touched.
//! An attachment a person or an agent made is never taken off (`source` is
//! `auto` on ours, NULL on theirs). It is idempotent, and runs on every PR
//! write, so it finds its rows with one indexed read
//! ([`crate::db::Db::auto_attach`]) and loads no board. A PR closed without
//! merging stays attached: the lane rule says where its row is.

use arugula_control_wire::boards as wire;

use super::{
    changed,
    refs::{Refs, references},
};
use crate::{Platform, db::boards::PrText};

/// The PRs of `repo` with these `numbers`, if it is linked to a project:
/// each attached to the rows it names, and the boards that moved told.
pub fn auto_attach_prs(app: &Platform, repo: u64, numbers: &[u64]) {
    if numbers.is_empty() {
        return;
    }
    let project = match app.db.linked_repo(repo) {
        Ok(Some(l)) if l.unlinked_at.is_none() => l.project_id,
        Ok(_) => return,
        Err(e) => return tracing::warn!(error = %e, "boards: finding the project of a repo"),
    };
    match app.db.prs_to_attach(repo, numbers) {
        Ok(prs) => prs.iter().for_each(|pr| auto_attach(app, &project, pr)),
        Err(e) => tracing::warn!(error = %e, "boards: reading PRs to attach"),
    }
}

/// Attach `pr` (a PR of a repo linked to `project`) to the rows of
/// `project`'s boards whose issue it names, and take it off those it
/// doesn't, then tell the boards that moved.
pub fn auto_attach(app: &Platform, project: &str, pr: &PrText) {
    let refs =
        references(&pr.title, pr.head_ref.as_deref().unwrap_or(""), pr.body.as_deref().unwrap_or(""), Some(pr.number));
    match attach(app, project, pr, &refs) {
        Ok(moved) => moved.iter().for_each(|b| changed(app, project, b)),
        Err(e) => tracing::warn!(error = %e, "boards: attaching a PR to the rows it names"),
    }
}

fn attach(app: &Platform, project: &str, pr: &PrText, refs: &Refs) -> anyhow::Result<Vec<String>> {
    let mut plane = vec![];
    if !refs.plane.is_empty() {
        for c in app.db.tracker_connections(project)?.iter().filter(|c| c.unlinked_at.is_none()) {
            for (_, n) in refs.plane.iter().filter(|(k, _)| k.eq_ignore_ascii_case(&c.identifier)) {
                plane.push((c.mirror_id, *n));
            }
        }
    }
    let done =
        app.db.auto_attach(project, &pr.id, pr.repo_id, refs, &plane, arugula_e2e::now_ms(), wire::ATTACHMENTS_MAX)?;
    let mut moved = done.attached;
    for b in done.detached {
        if !moved.contains(&b) {
            moved.push(b);
        }
    }
    Ok(moved)
}
