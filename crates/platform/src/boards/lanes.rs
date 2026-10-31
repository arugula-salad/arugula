//! A row's lane, and whether a Done row is proven (aru-125). Both are worked
//! out on every read from the live state of the row's items, by pure
//! functions of facts, and never stored. The facts are the state of an item
//! whoever may read it: a lane moves for a reader who can't see the PR that
//! moved it, and the attachment's chip says nothing to them ([`super`]).
//!
//! The lane is the furthest that applies, in the order Done, Review, Doing,
//! Next:
//!
//! | the row's items                                                  | lane   |
//! |------------------------------------------------------------------|--------|
//! | a note: no issue or PR as its item                               | Next   |
//! | the primary issue is completed or canceled, or the primary PR merged or closed | Done |
//! | the primary PR, or an attached PR, is open                       | Review |
//! | the primary issue is open, an attached PR merged, none is open   | Review |
//! | the primary issue is started, and its state's name says review   | Review |
//! | the primary issue is started                                     | Doing  |
//! | an attached agent block that isn't closed, or a run running      | Doing  |
//! | otherwise (triage, backlog or unstarted)                         | Next   |
//!
//! Only the primary item's category counts: an attached issue never moves a
//! lane. A started state is Review when "review" is in its name
//! ([`names_review`]), which only a Plane item has.
//!
//! A canceled primary issue (a Plane `cancelled` state, a native issue closed
//! as not planned or as a duplicate) is Done and never proven: it has the
//! `cancelled` mark, whatever merged.
//!
//! Merged isn't done (decision 7): a PR that merged while its issue is open
//! leaves the row in Review, marked `merged_not_closed`
//! ([`merged_not_closed`]), until the issue is closed. A closed block no
//! longer counts toward Doing, so a replaced agent leaves the row where the
//! work puts it.
//! **Proven** is a mark on a Done row: by CI when the primary PR or an
//! attached one merged with every check on its head completed and none red;
//! by evidence when a `proof` attachment is on it; otherwise not proven.

use arugula_control_wire::boards::{Category, Lane, Pointer, Proof, ProofBy};

/// A PR as a lane reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pr {
    Open,
    /// `green`: every check on its head completed, none red (and there is
    /// at least one).
    Merged {
        green: bool,
    },
    /// Closed unmerged.
    Closed,
}

/// What an item is now, as far as a lane cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fact {
    /// An issue of any source, by its status category.
    Issue {
        category: Category,
        /// A started Plane state whose name says review ([`names_review`]).
        review: bool,
    },
    Pr(Pr),
    /// An agent block, as its machine last reported it.
    Block {
        closed: bool,
    },
    Run {
        running: bool,
    },
}

/// A row's primary item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primary<'a> {
    /// No issue or PR: a note, which no state moves.
    Note,
    /// An issue or a PR, with its state if it is known (an item the mirror
    /// doesn't have has none).
    Item(Option<&'a Fact>),
}

/// The row's lane.
pub fn lane(primary: Primary<'_>, attached: &[Fact]) -> Lane {
    let Primary::Item(item) = primary else { return Lane::Next };
    match item {
        Some(Fact::Issue { category, .. }) if category.is_done() => return Lane::Done,
        Some(Fact::Pr(Pr::Merged { .. } | Pr::Closed)) => return Lane::Done,
        _ => {}
    }
    let open = |f: &Fact| matches!(f, Fact::Pr(Pr::Open));
    if item.is_some_and(open) || attached.iter().any(open) || merged_not_closed(primary, attached) {
        return Lane::Review;
    }
    let started = matches!(item, Some(Fact::Issue { category: Category::Started, .. }));
    if matches!(item, Some(Fact::Issue { review: true, .. })) {
        return Lane::Review;
    }
    let under_way = |f: &Fact| matches!(f, Fact::Block { closed: false } | Fact::Run { running: true });
    if started || attached.iter().any(under_way) { Lane::Doing } else { Lane::Next }
}

/// Whether a started Plane state's name says review: "In Review" and
/// "Code review" do, whatever the case.
pub fn names_review(state_name: &str) -> bool {
    state_name.to_ascii_lowercase().contains("review")
}

/// Whether the row's primary item is canceled, from any source: the row is
/// Done, never proven, and marked `cancelled`.
pub fn cancelled(primary: Primary<'_>) -> bool {
    matches!(primary, Primary::Item(Some(Fact::Issue { category: Category::Canceled, .. })))
}

/// A native issue's category: open is `unstarted`, closed as not planned or
/// as a duplicate is `canceled`, any other close (or none named) `completed`.
pub fn native_category(closed: bool, reason: Option<&str>) -> Category {
    match (closed, reason) {
        (false, _) => Category::Unstarted,
        (true, Some("not_planned" | "duplicate")) => Category::Canceled,
        (true, _) => Category::Completed,
    }
}

/// A mirrored GitHub issue's category from its `state` and `state_reason`:
/// closed as not planned (or as a duplicate) is `canceled`, any other close
/// `completed`. A row mirrored before the reason was kept has none, and reads
/// as `completed` until a re-sync fills it.
pub fn github_category(state: &str, reason: Option<&str>) -> Category {
    match (state, reason) {
        ("open", _) => Category::Unstarted,
        (_, Some("not_planned" | "duplicate")) => Category::Canceled,
        _ => Category::Completed,
    }
}

/// A Plane item's category: its state group, which spells `canceled` as
/// `cancelled`. A group this build doesn't know, or none, reads as the
/// item's `state`, open or closed, as a GitHub issue does.
pub fn plane_category(group: Option<&str>, state: &str) -> Category {
    match group.map(str::to_ascii_lowercase).as_deref() {
        Some("triage") => Category::Triage,
        Some("backlog") => Category::Backlog,
        Some("unstarted") => Category::Unstarted,
        Some("started") => Category::Started,
        Some("completed") => Category::Completed,
        Some("cancelled" | "canceled") => Category::Canceled,
        _ => github_category(state, None),
    }
}

/// When a Doing row stalled (Arugula #404), in milliseconds: every block of
/// it that isn't closed is idle (`idle[i]` is when the i-th went idle, none
/// for one that is not), the last of them went idle more than `minutes` ago,
/// and no PR (or running run) is on it. The time is when that last block
/// went idle. `attached` are the row's work facts, `idle` has one entry for
/// each open block among them.
pub fn stalled(lane: Lane, attached: &[Fact], idle: &[Option<u64>], now: u64, minutes: u32) -> Option<u64> {
    if lane != Lane::Doing || idle.is_empty() || idle.iter().any(Option::is_none) {
        return None;
    }
    if attached.iter().any(|f| matches!(f, Fact::Pr(_) | Fact::Run { running: true })) {
        return None;
    }
    let since = idle.iter().flatten().copied().max()?;
    (now.saturating_sub(since) > u64::from(minutes) * 60_000).then_some(since)
}

/// Whether the row's issue is open while a PR of the row merged and none is
/// open: the work is in and the issue isn't closed.
pub fn merged_not_closed(primary: Primary<'_>, attached: &[Fact]) -> bool {
    let Primary::Item(Some(Fact::Issue { category, .. })) = primary else { return false };
    if category.is_done() {
        return false;
    }
    let merged = |f: &Fact| matches!(f, Fact::Pr(Pr::Merged { .. }));
    attached.iter().any(merged) && !attached.iter().any(|f| matches!(f, Fact::Pr(Pr::Open)))
}

/// Whether a Done row is proven, and how. `item`: the primary pointer and its
/// fact; `attached`: the PRs attached as work, as (pointer, fact);
/// `evidence`: the first `proof` attachment. A canceled row is never proven.
pub fn proof(
    item: (&Pointer, Option<&Fact>),
    attached: &[(&Pointer, Fact)],
    evidence: Option<&Pointer>,
) -> Option<Proof> {
    if cancelled(Primary::Item(item.1)) {
        return None;
    }
    let green = |f: &Fact| matches!(f, Fact::Pr(Pr::Merged { green: true }));
    let by_ci = item
        .1
        .filter(|f| green(f))
        .map(|_| item.0)
        .or_else(|| attached.iter().find(|(_, f)| green(f)).map(|(p, _)| *p));
    match (by_ci, evidence) {
        (Some(p), _) => Some(Proof { by: ProofBy::Ci, pointer: Some(p.clone()) }),
        (None, Some(p)) => Some(Proof { by: ProofBy::Evidence, pointer: Some(p.clone()) }),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN_PR: Fact = Fact::Pr(Pr::Open);
    const MERGED: Fact = Fact::Pr(Pr::Merged { green: true });
    const LIVE_BLOCK: Fact = Fact::Block { closed: false };
    const RUNNING: Fact = Fact::Run { running: true };

    fn lane_of(primary: Primary<'_>, attached: &[Fact]) -> Lane {
        lane(primary, attached)
    }

    /// Every row of the lane table.
    #[test]
    fn the_lane_table() {
        let issue_open = Fact::Issue { category: Category::Unstarted, review: false };
        let issue_closed = Fact::Issue { category: Category::Completed, review: false };
        fn item(f: &Fact) -> Primary<'_> {
            Primary::Item(Some(f))
        }
        // A note: no state moves it.
        assert_eq!(lane_of(Primary::Note, &[OPEN_PR, LIVE_BLOCK, RUNNING]), Lane::Next);
        // Done: the issue is closed (any reason), or the PR merged or closed.
        assert_eq!(lane_of(item(&issue_closed), &[]), Lane::Done);
        assert_eq!(lane_of(item(&issue_closed), &[OPEN_PR, LIVE_BLOCK]), Lane::Done);
        assert_eq!(lane_of(item(&MERGED), &[]), Lane::Done);
        assert_eq!(lane_of(item(&Fact::Pr(Pr::Merged { green: false })), &[]), Lane::Done);
        assert_eq!(lane_of(item(&Fact::Pr(Pr::Closed)), &[LIVE_BLOCK]), Lane::Done);
        // Review: the primary PR, or an attached one, is open.
        assert_eq!(lane_of(item(&OPEN_PR), &[]), Lane::Review);
        assert_eq!(lane_of(item(&OPEN_PR), &[LIVE_BLOCK, RUNNING]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[OPEN_PR]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[LIVE_BLOCK, OPEN_PR]), Lane::Review);
        // Doing: a block that isn't closed, or a run running, and no open PR.
        assert_eq!(lane_of(item(&issue_open), &[LIVE_BLOCK]), Lane::Doing);
        assert_eq!(lane_of(item(&issue_open), &[RUNNING]), Lane::Doing);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Pr(Pr::Closed), LIVE_BLOCK]), Lane::Doing);
        // Next: otherwise.
        assert_eq!(lane_of(item(&issue_open), &[]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Block { closed: true }]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Run { running: false }]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Pr(Pr::Closed)]), Lane::Next);
        // Merged isn't done: the issue is open and a PR of it merged, none
        // open. Review, over a live block; an open PR is Review anyway.
        assert_eq!(lane_of(item(&issue_open), &[MERGED]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Pr(Pr::Merged { green: false })]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Pr(Pr::Closed), MERGED]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[MERGED, LIVE_BLOCK]), Lane::Review);
        assert_eq!(lane_of(item(&issue_open), &[MERGED, OPEN_PR]), Lane::Review);
        // ...until the issue is closed: then Done, whatever else is attached.
        assert_eq!(lane_of(item(&issue_closed), &[MERGED]), Lane::Done);
        // A merged PR says nothing of a row whose issue isn't known, or a note.
        assert_eq!(lane_of(Primary::Item(None), &[MERGED]), Lane::Next);
        assert_eq!(lane_of(Primary::Note, &[MERGED]), Lane::Next);
        // An item the mirror doesn't know says nothing; its attachments do.
        assert_eq!(lane_of(Primary::Item(None), &[]), Lane::Next);
        assert_eq!(lane_of(Primary::Item(None), &[OPEN_PR]), Lane::Review);
    }

    /// The mark `merged_not_closed`: an open issue, a merged PR, no open one.
    #[test]
    fn merged_not_closed_needs_an_open_issue_a_merged_pr_and_no_open_one() {
        let (open, closed) = (
            Fact::Issue { category: Category::Unstarted, review: false },
            Fact::Issue { category: Category::Completed, review: false },
        );
        let mnc = |p: Primary<'_>, a: &[Fact]| merged_not_closed(p, a);
        assert!(mnc(Primary::Item(Some(&open)), &[MERGED]));
        assert!(mnc(Primary::Item(Some(&open)), &[Fact::Pr(Pr::Closed), MERGED, LIVE_BLOCK]));
        assert!(!mnc(Primary::Item(Some(&open)), &[]), "nothing merged");
        assert!(!mnc(Primary::Item(Some(&open)), &[MERGED, OPEN_PR]), "another is open");
        assert!(!mnc(Primary::Item(Some(&open)), &[Fact::Pr(Pr::Closed)]), "closed unmerged is not merged");
        assert!(!mnc(Primary::Item(Some(&closed)), &[MERGED]), "the issue is closed: Done");
        assert!(!mnc(Primary::Item(Some(&MERGED)), &[MERGED]), "a PR primary is Done at once");
        assert!(!mnc(Primary::Item(None), &[MERGED]));
        assert!(!mnc(Primary::Note, &[MERGED]));
    }

    /// A Doing row is stalled when all its open blocks have been idle for
    /// longer than N minutes and nothing else is on it.
    #[test]
    fn a_doing_row_stalls_when_every_open_block_is_idle_past_n_minutes() {
        const MIN: u64 = 60_000;
        let now = 100 * MIN;
        let st = |lane, attached: &[Fact], idle: &[Option<u64>]| stalled(lane, attached, idle, now, 30);
        let blocks = [LIVE_BLOCK, LIVE_BLOCK];
        // Idle for 42 minutes: stalled, since then.
        assert_eq!(st(Lane::Doing, &[LIVE_BLOCK], &[Some(58 * MIN)]), Some(58 * MIN));
        // Exactly N is not longer than N; N and a millisecond is.
        assert_eq!(st(Lane::Doing, &[LIVE_BLOCK], &[Some(70 * MIN)]), None);
        assert_eq!(st(Lane::Doing, &[LIVE_BLOCK], &[Some(70 * MIN - 1)]), Some(70 * MIN - 1));
        // One block still working (or waiting): not stalled.
        assert_eq!(st(Lane::Doing, &blocks, &[Some(10 * MIN), None]), None);
        // The last to go idle counts.
        assert_eq!(st(Lane::Doing, &blocks, &[Some(10 * MIN), Some(50 * MIN)]), Some(50 * MIN));
        assert_eq!(st(Lane::Doing, &blocks, &[Some(10 * MIN), Some(90 * MIN)]), None);
        // A PR on the row (any state), or a run running: not stalled.
        assert_eq!(st(Lane::Doing, &[LIVE_BLOCK, Fact::Pr(Pr::Closed)], &[Some(0)]), None);
        assert_eq!(st(Lane::Doing, &[LIVE_BLOCK, RUNNING], &[Some(0)]), None);
        // Only a Doing row; no open block says nothing.
        assert_eq!(st(Lane::Review, &[LIVE_BLOCK], &[Some(0)]), None);
        assert_eq!(st(Lane::Doing, &[RUNNING], &[]), None);
    }

    fn ptr(id: &str) -> Pointer {
        Pointer::ForgeItem { id: id.into() }
    }

    /// Done is not proven: CI proves it from a green merge (the primary PR's
    /// or an attached one's), evidence from a `proof` attachment.
    #[test]
    fn a_done_row_is_proven_by_ci_or_evidence_or_not() {
        let (pr, att) = (ptr("fi_pr"), ptr("fi_att"));
        let link = Pointer::Link { url: "https://ci.example.com/1".into() };
        let issue = Fact::Issue { category: Category::Completed, review: false };
        let red = Fact::Pr(Pr::Merged { green: false });
        let ci = |p: &Pointer| Some(Proof { by: ProofBy::Ci, pointer: Some(p.clone()) });
        // The primary PR merged green.
        assert_eq!(proof((&pr, Some(&MERGED)), &[], None), ci(&pr));
        // The primary issue closed, an attached PR merged green.
        let none = ptr("iss_1");
        assert_eq!(proof((&none, Some(&issue)), &[(&att, MERGED)], None), ci(&att));
        // A merge with a red or unfinished check isn't proven by CI.
        assert_eq!(proof((&pr, Some(&red)), &[], None), None);
        assert_eq!(proof((&none, Some(&issue)), &[(&att, red)], None), None);
        // Evidence is a mark of its own, and CI wins when both are there.
        let by_hand = Some(Proof { by: ProofBy::Evidence, pointer: Some(link.clone()) });
        assert_eq!(proof((&none, Some(&issue)), &[], Some(&link)), by_hand);
        assert_eq!(proof((&pr, Some(&red)), &[], Some(&link)), by_hand);
        assert_eq!(proof((&pr, Some(&MERGED)), &[], Some(&link)), ci(&pr));
        // Closed unmerged is Done, and never proven by CI.
        assert_eq!(proof((&pr, Some(&Fact::Pr(Pr::Closed))), &[], None), None);
    }

    /// Each source's status category: a native issue by how it closed, a
    /// GitHub issue by open or closed (the mirror has no reason), Plane's
    /// state group as it is with `cancelled` spelled `canceled`, and a group
    /// we don't know by the item's state.
    #[test]
    fn every_source_maps_onto_a_status_category() {
        use Category::*;
        assert_eq!(native_category(false, None), Unstarted);
        assert_eq!(native_category(true, Some("completed")), Completed);
        assert_eq!(native_category(true, None), Completed);
        assert_eq!(native_category(true, Some("not_planned")), Canceled);
        assert_eq!(native_category(true, Some("duplicate")), Canceled);
        assert_eq!(github_category("open", None), Unstarted);
        assert_eq!(github_category("open", Some("reopened")), Unstarted);
        assert_eq!(github_category("closed", None), Completed);
        assert_eq!(github_category("closed", Some("completed")), Completed);
        assert_eq!(github_category("closed", Some("not_planned")), Canceled);
        assert_eq!(github_category("closed", Some("duplicate")), Canceled);
        let plane = |group: &str| plane_category(Some(group), "open");
        assert_eq!(plane("triage"), Triage);
        assert_eq!(plane("backlog"), Backlog);
        assert_eq!(plane("unstarted"), Unstarted);
        assert_eq!(plane("started"), Started);
        assert_eq!(plane("completed"), Completed);
        assert_eq!(plane("cancelled"), Canceled);
        // Not a group we know, or none: by the item's state.
        assert_eq!(plane_category(Some("someday"), "open"), Unstarted);
        assert_eq!(plane_category(Some("someday"), "closed"), Completed);
        assert_eq!(plane_category(None, "open"), Unstarted);
        assert_eq!(plane_category(None, "closed"), Completed);
        assert_eq!(serde_json::to_string(&Canceled).unwrap(), "\"canceled\"");
    }

    /// Only completed and canceled are Done; every other category is open.
    #[test]
    fn completed_and_canceled_are_done_and_the_rest_are_not() {
        use Category::*;
        for c in [Triage, Backlog, Unstarted, Started] {
            let f = Fact::Issue { category: c, review: false };
            let alone = if c == Started { Lane::Doing } else { Lane::Next };
            assert_eq!(lane(Primary::Item(Some(&f)), &[]), alone, "{c:?}");
            assert_eq!(lane(Primary::Item(Some(&f)), &[OPEN_PR]), Lane::Review, "{c:?}");
            assert_eq!(lane(Primary::Item(Some(&f)), &[LIVE_BLOCK]), Lane::Doing, "{c:?}");
            assert!(merged_not_closed(Primary::Item(Some(&f)), &[MERGED]), "{c:?}");
        }
        for c in [Completed, Canceled] {
            let f = Fact::Issue { category: c, review: false };
            assert_eq!(lane(Primary::Item(Some(&f)), &[OPEN_PR, LIVE_BLOCK]), Lane::Done, "{c:?}");
            assert!(!merged_not_closed(Primary::Item(Some(&f)), &[MERGED]), "{c:?}");
        }
    }

    /// A Plane started state is Doing, or Review when its name says so; the
    /// states before it are Next; an attached PR still lifts to Review and a
    /// merged one with the issue open stays Review over a started primary.
    #[test]
    fn a_started_primary_is_doing_or_review_and_the_rest_wait() {
        let issue = |category, review| Fact::Issue { category, review };
        let lane_of = |f: &Fact, a: &[Fact]| lane(Primary::Item(Some(f)), a);
        let started = issue(Category::Started, false);
        assert_eq!(lane_of(&started, &[]), Lane::Doing);
        assert_eq!(lane_of(&issue(Category::Started, true), &[]), Lane::Review);
        assert_eq!(lane_of(&issue(Category::Started, true), &[LIVE_BLOCK]), Lane::Review);
        assert_eq!(lane_of(&started, &[OPEN_PR]), Lane::Review);
        assert_eq!(lane_of(&started, &[MERGED]), Lane::Review, "merged, not closed stays in Review");
        assert!(merged_not_closed(Primary::Item(Some(&started)), &[MERGED]));
        for c in [Category::Triage, Category::Backlog, Category::Unstarted] {
            assert_eq!(lane_of(&issue(c, false), &[]), Lane::Next, "{c:?}");
        }
        // Only the primary counts: an attached issue moves nothing.
        assert_eq!(lane_of(&issue(Category::Unstarted, false), &[started]), Lane::Next);
        // Done wins over a review name.
        assert_eq!(lane_of(&issue(Category::Completed, true), &[]), Lane::Done);
    }

    #[test]
    fn a_state_name_with_review_in_it_is_review() {
        assert!(names_review("In Review"));
        assert!(names_review("code REVIEW"));
        assert!(names_review("Peer-reviewing"));
        assert!(!names_review("In Progress"));
        assert!(!names_review(""));
    }

    /// A canceled primary is Done and never proven, even with a PR that
    /// merged green; the mark is the primary's alone.
    #[test]
    fn a_canceled_primary_is_done_and_cancelled() {
        let canceled = Fact::Issue { category: Category::Canceled, review: false };
        let done = Fact::Issue { category: Category::Completed, review: false };
        assert_eq!(lane(Primary::Item(Some(&canceled)), &[MERGED, OPEN_PR, LIVE_BLOCK]), Lane::Done);
        assert!(!merged_not_closed(Primary::Item(Some(&canceled)), &[MERGED]));
        assert!(cancelled(Primary::Item(Some(&canceled))));
        let (none, att) = (ptr("iss_1"), ptr("fi_att"));
        let link = Pointer::Link { url: "https://ci.example.com/1".into() };
        assert_eq!(proof((&none, Some(&canceled)), &[(&att, MERGED)], Some(&link)), None);
        assert!(!cancelled(Primary::Item(Some(&done))));
        assert!(!cancelled(Primary::Item(Some(&MERGED))));
        assert!(!cancelled(Primary::Item(None)));
        assert!(!cancelled(Primary::Note));
    }
}
