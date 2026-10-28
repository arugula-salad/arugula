//! Gates waiting for a person (M34), whoever read them: the `gate` reason
//! the swarm's rail, push and the phone show. Approving one through where
//! it came from is Labs' (`labs/gates.rs`). The workspace block reads chant's; a studio app
//! block (M35) reads hud's; a forge block (M36) raises one for a review
//! asked of you. Neither the reason nor the card knows which. A workspace
//! decision point's open question (#621) rides the same way, answered with
//! one of its choices rather than approved.

use arugula_proto::{Action, Gate, GateSource, Reason, ReasonKind};

use crate::store::now_ms;

/// Why a block with these gates waiting wants you: the first gate, with
/// how many more, bundled by its workspace. `allow` approves it; `expire`
/// turns a chant gate down (#310). hud has no route for that, and a review
/// asked of you is turned down on its forge block (request changes), so
/// theirs don't offer it. A decision point's question is answered
/// (`answer`, with one of its choices).
pub fn reason(gates: &[Gate]) -> Option<Reason> {
    let g = gates.first()?;
    let actions = match g.source {
        GateSource::Chant { .. } => vec![Action::Allow, Action::Expire, Action::Dismiss],
        GateSource::Point { .. } => vec![Action::Answer, Action::Dismiss],
        _ => vec![Action::Allow, Action::Dismiss],
    };
    let more = if gates.len() > 1 { format!(" (+{} more)", gates.len() - 1) } else { String::new() };
    Some(Reason {
        kind: ReasonKind::Gate,
        since_ms: g.since.as_deref().and_then(rfc3339_ms).unwrap_or_else(now_ms),
        headline: format!("{}{more}", g.headline()),
        command: g.command.clone(),
        exit: None,
        duration_ms: None,
        bundle: Some(g.bundle()),
        ask: None,
        gate: Some(Box::new(g.clone())),
        diff: None,
        actions,
    })
}

/// Approve `gate` as `approver` (#75: the owner or an editor, by their
/// Arugula name), through its source. What the source said, on success.
/// What a source needs to approve one of its gates.
/// Text without terminal colours, trimmed.
/// `2026-10-02T21:09:51.548Z` as ms since the epoch (UTC only, as chant
/// writes them).
pub(crate) fn rfc3339_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || !s.ends_with('Z') {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = match s.get(19..s.len() - 1) {
        Some(f) if f.starts_with('.') => format!("{:0<3}", &f[1..f.len().min(4)]).parse::<i64>().ok()?,
        _ => 0,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    u64::try_from(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(member: &str) -> Gate {
        Gate {
            member: member.into(),
            op: "ship".into(),
            gate: "approve-ship".into(),
            env: None,
            since: Some("2026-10-02T21:09:51.548Z".into()),
            expires: None,
            approvals: 0,
            needed: 1,
            command: Some("chant approve ship approve-ship".into()),
            source: GateSource::Chant { root: "/w".into(), dir: format!("/w/{member}"), machine: None },
            why: None,
        }
    }

    #[test]
    fn a_reason_from_gates() {
        assert_eq!(reason(&[]), None);
        let r = reason(&[gate("delivery"), gate("app")]).unwrap();
        assert_eq!(r.kind, ReasonKind::Gate);
        assert_eq!(r.headline, "delivery: ship waits at gate approve-ship (+1 more)");
        assert_eq!(r.bundle.as_deref(), Some("gate:/w"));
        assert_eq!(r.since_ms, 1_790_975_391_548);
        assert_eq!(r.gate.unwrap().key(), "delivery/ship/approve-ship");
        assert_eq!(r.actions, [Action::Allow, Action::Expire, Action::Dismiss]);
        // hud's and a forge's: no expire.
        let mut hud = gate("delivery");
        hud.source = GateSource::Hud { box_url: "https://box".into(), app: "a".into() };
        assert_eq!(reason(&[hud]).unwrap().actions, [Action::Allow, Action::Dismiss]);
        let one = reason(&[gate("delivery")]).unwrap();
        assert_eq!(one.headline, "delivery: ship waits at gate approve-ship");
    }

    fn question() -> Gate {
        use arugula_proto::workspace::PointChoice;
        let choice = |v: &str| PointChoice { value: v.into(), label: v.into(), means: None };
        Gate {
            member: String::new(),
            op: "slice-tier".into(),
            gate: "slice-tier-1724faf3af79".into(),
            env: None,
            since: Some("2026-10-08T00:00:00Z".into()),
            expires: None,
            approvals: 0,
            needed: 1,
            command: None,
            source: GateSource::Point {
                root: "/w".into(),
                machine: None,
                id: "slice-tier-1724faf3af79".into(),
                point: "slice-tier".into(),
                question: "Which builder tier builds this work item (W-001)".into(),
                choices: vec![choice("small"), choice("medium"), choice("large")],
                proposed: None,
            },
            why: None,
        }
    }

    #[test]
    fn a_decision_points_question_is_answered() {
        let r = reason(&[question()]).unwrap();
        assert_eq!(r.kind, ReasonKind::Gate);
        assert_eq!(r.actions, [Action::Answer, Action::Dismiss]);
        assert_eq!(r.headline, "Which builder tier builds this work item (W-001)");
        assert_eq!(r.bundle.as_deref(), Some("gate:/w"));
        // With the workspace's gates on one card.
        let both = reason(&[gate("delivery"), question()]).unwrap();
        assert_eq!(both.headline, "delivery: ship waits at gate approve-ship (+1 more)");
    }

    #[test]
    fn times_and_text() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_500));
        assert_eq!(rfc3339_ms("yesterday"), None);
    }
}
