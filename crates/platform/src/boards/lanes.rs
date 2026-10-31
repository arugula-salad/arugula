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
//! | the primary issue is closed, or the primary PR merged or closed  | Done   |
//! | the primary PR, or an attached PR, is open                       | Review |
//! | the primary issue is open, an attached PR merged, none is open   | Review |
//! | an attached agent block that isn't closed, or a run running      | Doing  |
//! | otherwise                                                        | Next   |
//!
//! Merged isn't done (decision 7): a PR that merged while its issue is open
//! leaves the row in Review, marked `merged_not_closed`
//! ([`merged_not_closed`]), until the issue is closed. A closed block no
//! longer counts toward Doing, so a replaced agent leaves the row where the
//! work puts it.
//! **Proven** is a mark on a Done row: by CI when the primary PR or an
//! attached one merged with every check on its head completed and none red;
//! by evidence when a `proof` attachment is on it; otherwise not proven.

use arugula_control_wire::boards::{Lane, Pointer, Proof, ProofBy};

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
    Issue {
        closed: bool,
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
    if let Some(Fact::Issue { closed: true } | Fact::Pr(Pr::Merged { .. } | Pr::Closed)) = item {
        return Lane::Done;
    }
    let open = |f: &Fact| matches!(f, Fact::Pr(Pr::Open));
    if item.is_some_and(open) || attached.iter().any(open) || merged_not_closed(primary, attached) {
        return Lane::Review;
    }
    let under_way = |f: &Fact| matches!(f, Fact::Block { closed: false } | Fact::Run { running: true });
    if attached.iter().any(under_way) { Lane::Doing } else { Lane::Next }
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
    let Primary::Item(Some(Fact::Issue { closed: false })) = primary else { return false };
    let merged = |f: &Fact| matches!(f, Fact::Pr(Pr::Merged { .. }));
    attached.iter().any(merged) && !attached.iter().any(|f| matches!(f, Fact::Pr(Pr::Open)))
}

/// Whether a Done row is proven, and how. `item`: the primary pointer and its
/// fact; `attached`: the PRs attached as work, as (pointer, fact);
/// `evidence`: the first `proof` attachment.
pub fn proof(
    item: (&Pointer, Option<&Fact>),
    attached: &[(&Pointer, Fact)],
    evidence: Option<&Pointer>,
) -> Option<Proof> {
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
        let issue_open = Fact::Issue { closed: false };
        let issue_closed = Fact::Issue { closed: true };
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
        let (open, closed) = (Fact::Issue { closed: false }, Fact::Issue { closed: true });
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
        let issue = Fact::Issue { closed: true };
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
}
