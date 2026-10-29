//! A thread is done when its PR merges (M265): the mirror wrote a PR as
//! merged (the `pull_request` webhook's `closed` with `merged`, or the
//! poller's read), and the session whose branch it's from ends. The rules
//! are `db::session_merged`'s; this is the hook the mirror calls and the
//! cards it tells, and the project page's numbers ([`stats`]).

use std::sync::Arc;

use arugula_control_common::ApiError;
use arugula_control_wire::sessions::{STATS_DAYS, SessionStats};
use axum::{
    Json,
    extract::{Path, State},
};
use tracing::{info, warn};

use crate::{
    Platform,
    auth::Person,
    db::{mirror::ItemRow, session_merged::Counted},
    forge::links::membership,
};

const DAY_MS: u64 = 24 * 60 * 60_000;

/// The mirror wrote `items` of linked repo `repo_id`: each merged PR from
/// a session's branch marks that session merged, and stops it.
pub fn seen(app: &Platform, repo_id: u64, items: &[&ItemRow]) {
    let now = arugula_e2e::now_ms();
    for it in items.iter().filter(|i| i.kind == "pr" && i.state == "merged") {
        let Some(head) = it.head_ref.as_deref() else { continue };
        match app.db.session_merged(repo_id, head, it.number, now) {
            Ok((marked, stopped)) => {
                for id in &marked {
                    info!(
                        session = id,
                        repo = repo_id,
                        pr = it.number,
                        stopped = stopped.contains(id),
                        "a session's PR merged"
                    );
                }
                if let Err(e) = crate::session_live::told(app, &marked) {
                    warn!(repo = repo_id, error = format!("{e:#}"), "telling merged sessions' cards");
                }
            }
            Err(e) => warn!(repo = repo_id, pr = it.number, error = format!("{e:#}"), "marking a session's PR merged"),
        }
    }
}

/// The numbers of `counted` (sessions asked for since a time).
pub fn numbers(counted: &[Counted]) -> SessionStats {
    let mut took: Vec<u64> = counted.iter().filter_map(|c| c.pr_at.map(|at| at.saturating_sub(c.asked_at))).collect();
    took.sort_unstable();
    let median = match took.len() {
        0 => None,
        n if n % 2 == 1 => Some(took[n / 2]),
        n => Some((took[n / 2 - 1] + took[n / 2]) / 2),
    };
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    SessionStats {
        days: STATS_DAYS,
        sessions: count(counted.len()),
        prs_opened: count(took.len()),
        prs_merged: count(counted.iter().filter(|c| c.merged_at.is_some()).count()),
        median_to_pr_ms: median,
    }
}

/// `GET /api/projects/{project}/sessions/stats`, for its members: of the
/// sessions asked for in the last [`STATS_DAYS`] days, how many led to a
/// PR and how many merged, and the median time from the `@` to the PR.
pub async fn stats(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<SessionStats>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    let since = arugula_e2e::now_ms().saturating_sub(u64::from(STATS_DAYS) * DAY_MS);
    Ok(Json(numbers(&app.db.session_numbers(&project, since)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(asked_at: u64, pr_at: Option<u64>, merged: bool) -> Counted {
        Counted { session: format!("ses_{asked_at}"), asked_at, pr_at, merged_at: merged.then_some(asked_at + 1) }
    }

    /// The median of an even count is the mean of the middle two; a
    /// session with no PR counts as a session only.
    #[test]
    fn a_projects_numbers() {
        let s = numbers(&[
            counted(0, Some(60_000), true),
            counted(10, None, false),
            counted(20, Some(20 + 180_000), false),
            counted(30, Some(30 + 120_000), true),
            counted(40, Some(40 + 600_000), false),
        ]);
        assert_eq!((s.sessions, s.prs_opened, s.prs_merged), (5, 4, 2));
        assert_eq!(s.median_to_pr_ms, Some(150_000));
        assert_eq!(numbers(&[]).median_to_pr_ms, None);
    }
}
