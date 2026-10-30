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
//! | an attached agent block that isn't closed, or a run running      | Doing  |
//! | otherwise                                                        | Next   |
//!
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
    if item.is_some_and(open) || attached.iter().any(open) {
        return Lane::Review;
    }
    let under_way = |f: &Fact| matches!(f, Fact::Block { closed: false } | Fact::Run { running: true });
    if attached.iter().any(under_way) { Lane::Doing } else { Lane::Next }
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
        assert_eq!(lane_of(item(&issue_open), &[MERGED, LIVE_BLOCK]), Lane::Doing);
        // Next: otherwise.
        assert_eq!(lane_of(item(&issue_open), &[]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Block { closed: true }]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Run { running: false }]), Lane::Next);
        assert_eq!(lane_of(item(&issue_open), &[Fact::Pr(Pr::Closed), MERGED]), Lane::Next);
        // An item the mirror doesn't know says nothing; its attachments do.
        assert_eq!(lane_of(Primary::Item(None), &[]), Lane::Next);
        assert_eq!(lane_of(Primary::Item(None), &[OPEN_PR]), Lane::Review);
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
