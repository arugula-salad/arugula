//! Gates waiting for a person (M34), whoever read them: the `gate` reason
//! the swarm's rail, push and the phone show, and approving one through
//! where it came from. The workspace block reads chant's; a studio app
//! block (M35) reads hud's; a forge block (M36) raises one for a review
//! asked of you. Neither the reason nor the card knows which. A workspace
//! decision point's open question (#621) rides the same way, answered with
//! one of its choices rather than approved.

use arugula_proto::{Action, Gate, GateSource, Reason, ReasonKind};

#[cfg(feature = "labs")]
use crate::review::Runner;
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
        actions,
    })
}

#[cfg(feature = "labs")]
/// Turn `gate` down (#310): `chant approve <op> <gate> --expire` in the
/// member's directory, which clears its pending fact without approving
/// it, so the next run decides it from scratch. chant gates only.
pub async fn expire(gate: &Gate, via: &Via<'_>) -> Result<String, String> {
    match (&gate.source, via) {
        (GateSource::Chant { dir, .. }, Via::Chant { runner, chant, .. }) => {
            let mut args = vec!["approve".to_owned()];
            args.extend(expire_args(gate));
            run_chant(runner, chant, dir, args).await
        }
        (GateSource::Hud { .. }, _) => Err("hud has no way to expire a gate: dismiss it here".into()),
        (GateSource::Forge { .. }, _) => Err("a review is turned down on its forge block (request changes)".into()),
        (GateSource::Point { .. }, _) => Err(POINT_NOT_GATE.into()),
        (GateSource::Chant { .. }, Via::Hud { .. }) => Err("this gate isn't this block's to expire".into()),
    }
}

#[cfg(feature = "labs")]
/// `<op> <gate> [--env E] --expire`: the environment as status's line
/// names it (it expires that environment's pending fact).
pub fn expire_args(gate: &Gate) -> Vec<String> {
    let mut args = vec![gate.op.clone(), gate.gate.clone()];
    if let Some(env) = &gate.env {
        args.extend(["--env".into(), env.clone()]);
    }
    args.push("--expire".into());
    args
}

/// Approve `gate` as `approver` (#75: the owner or an editor, by their
/// Arugula name), through its source. What the source said, on success.
#[cfg(feature = "labs")]
pub async fn approve(gate: &Gate, approver: Option<&str>, via: &Via<'_>) -> Result<String, String> {
    match (&gate.source, via) {
        (GateSource::Chant { dir, .. }, Via::Chant { runner, chant, by }) => {
            // Status's own `chant approve` line (#302), as the principal
            // chant records, in the member's directory: chant finds the
            // member's ledger from there.
            let mut args = vec!["approve".to_owned()];
            args.extend(chant_args(gate, by)?);
            run_chant(runner, chant, dir, args).await
        }
        (GateSource::Hud { .. }, Via::Hud { session, follower }) => {
            // hud re-reads `workspace status` and approves only a gate
            // pending there, with `--actor` its roster name, or with a
            // follower credential, the person named here.
            let mut body = serde_json::json!({
                "member": gate.member, "component": gate.op, "gate": gate.gate, "env": gate.env,
            });
            if *follower && let Some(name) = approver.and_then(crate::labs::apps::hud::hud_name) {
                body["onBehalfOf"] = serde_json::json!({ "name": name, "via": "arugula" });
            }
            session.approve_gate(&body).await
        }
        // M36: a review asked of you is approved as a review, by the forge
        // block itself (`act` calls its `review` method).
        (GateSource::Forge { .. }, _) => Err("a review is approved through its forge block".into()),
        (GateSource::Point { .. }, _) => Err(POINT_NOT_GATE.into()),
        (GateSource::Chant { .. }, Via::Hud { .. }) | (GateSource::Hud { .. }, Via::Chant { .. }) => {
            Err("this gate isn't this block's to approve".into())
        }
    }
}

#[cfg(feature = "labs")]
/// Why a decision point's question isn't approved or expired.
const POINT_NOT_GATE: &str = "a decision point is answered, with one of its choices: it isn't approved or expired";

#[cfg(feature = "labs")]
/// Answer a decision point's open question (#621) with `value`, one of its
/// choices: `chant workspace points answer <id> --answer <value> --by
/// <principal>` in the workspace's root, as `by` (`--relayed-by` the owner
/// when someone else answers). chant counts the answer toward the point's
/// quorum and writes the answer record. What chant said, on success.
pub async fn answer(gate: &Gate, value: &str, via: &Via<'_>) -> Result<String, String> {
    match (&gate.source, via) {
        (GateSource::Point { root, .. }, Via::Chant { runner, chant, by }) => {
            let args = answer_args(gate, value, by)?;
            let said = run_chant(runner, chant, root, args).await?;
            Ok(points_said(&said).unwrap_or(said))
        }
        (GateSource::Point { .. }, Via::Hud { .. }) => Err("this question isn't this block's to answer".into()),
        _ => Err("a gate is approved, not answered".into()),
    }
}

#[cfg(feature = "labs")]
/// The `chant` arguments that answer `gate`'s question with `value`, as
/// `by`: a value its choices don't list (when they're known) isn't sent.
pub fn answer_args(gate: &Gate, value: &str, by: &ChantBy) -> Result<Vec<String>, String> {
    let GateSource::Point { id, choices, .. } = &gate.source else {
        return Err("a gate is approved, not answered".into());
    };
    if !choices.is_empty() && !choices.iter().any(|c| c.value == value) {
        let all: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
        return Err(format!("{value:?} isn't one of its answers ({})", all.join(", ")));
    }
    let who = by.actor.clone().ok_or(
        "chant records who answers, and nobody is named: set the owner's principal in the workspace block's principals",
    )?;
    let mut args = ["workspace", "points", "answer", id, "--answer", value, "--by", &who].map(str::to_owned).to_vec();
    if let Some(r) = by.relayed_by.as_ref().filter(|_| by.relayed) {
        args.extend(["--relayed-by".into(), r.clone()]);
    }
    Ok(args)
}

#[cfg(feature = "labs")]
/// What `points answer` printed on success, as a line: the question's new
/// title (`...: medium`). None when it isn't the points-write document.
fn points_said(said: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(said).ok()?;
    doc["question"]["title"].as_str().map(str::to_owned)
}

#[cfg(feature = "labs")]
/// `chant <args>` in `dir`: what it said, or why it failed.
async fn run_chant(runner: &Runner, chant: &str, dir: &str, args: Vec<String>) -> Result<String, String> {
    let script = r#"cd "$1" || exit 1; c=$2; shift 2; exec "$c" "$@" 2>&1"#;
    let what = args.iter().take_while(|a| !a.starts_with('-')).take(3).cloned().collect::<Vec<_>>().join(" ");
    let mut argv = vec![dir.to_owned(), chant.to_owned()];
    argv.extend(args);
    let (out, code) = runner.sh(script, &argv).await?;
    let said = plain(&String::from_utf8_lossy(&out));
    if code == Some(0) {
        return Ok(said);
    }
    if said.is_empty() {
        return Err(format!("chant {what} failed (exit {})", code.unwrap_or(-1)));
    }
    // `points answer` says why in its JSON document, with its code.
    let (why, code) = points_error(&said).unwrap_or_else(|| (said.clone(), String::new()));
    // ws-080: the workspace wants a forge identity or a signer.
    if code == "principal-unidentified"
        || said.contains("principal-unidentified")
        || said.contains("forge identity or signer")
    {
        return Err(format!(
            "{why}\n(name them for chant in the workspace block's principals, e.g. `arugula workspace --principal <name>=github:<login>`)"
        ));
    }
    Err(why)
}

#[cfg(feature = "labs")]
/// A points-write document's error: its message and code.
fn points_error(said: &str) -> Option<(String, String)> {
    let doc: serde_json::Value = serde_json::from_str(said).ok()?;
    let e = &doc["error"];
    Some((e["message"].as_str()?.to_owned(), e["code"].as_str().unwrap_or_default().to_owned()))
}

#[cfg(feature = "labs")]
/// Who approves a chant gate, as chant records them (ws-080).
#[derive(Debug, Clone, Default)]
pub struct ChantBy {
    /// `--actor`: the approver's principal (a forge identity such as
    /// `github:alice`, a signer, or a plain name where the workspace
    /// doesn't ask for more).
    pub actor: Option<String>,
    /// Someone other than the host's owner approves, so Arugula carries
    /// it: chant runs, and signs, as the owner.
    pub relayed: bool,
    /// `--relayed-by`: the owner's principal, when known and the chant
    /// takes it (0.103.1 and newer).
    pub relayed_by: Option<String>,
}

#[cfg(feature = "labs")]
/// The `chant approve` arguments for `gate`: status's own line (`approve`,
/// with `--plan <digest>` binding the approval to the plan read and
/// `--sign` for a gate in `identity.gates`), as `by`. A line that isn't
/// for this gate is rebuilt from what's known.
pub fn chant_args(gate: &Gate, by: &ChantBy) -> Result<Vec<String>, String> {
    let line = gate.command.as_deref().unwrap_or_default();
    let words: Vec<&str> = line.split_whitespace().collect();
    let flags: Vec<String> = match words.as_slice() {
        [c, "approve", op, g, rest @ ..] if c.ends_with("chant") && *op == gate.op && *g == gate.gate => {
            rest.iter().map(|w| (*w).to_owned()).collect()
        }
        _ => gate.env.iter().flat_map(|e| ["--env".to_owned(), e.clone()]).collect(),
    };
    if by.relayed && flags.iter().any(|f| f == "--sign") {
        return Err(format!(
            "gate {} needs a signed approval, sealed with the approver's own key, and chant runs here with the owner's: \
             the owner approves it here, or you run `{line} --actor <you>` where your key is",
            gate.gate
        ));
    }
    let mut args = vec![gate.op.clone(), gate.gate.clone()];
    if let Some(a) = &by.actor {
        args.extend(["--actor".into(), a.clone()]);
    }
    if let Some(r) = by.relayed_by.as_ref().filter(|_| by.relayed) {
        args.extend(["--relayed-by".into(), r.clone()]);
    }
    args.extend(flags);
    Ok(args)
}

/// What a source needs to approve one of its gates.
#[cfg(feature = "labs")]
pub enum Via<'a> {
    /// The workspace's chant, run on the block's host with the user's
    /// shell environment, as `by`.
    Chant { runner: &'a Runner, chant: &'a str, by: ChantBy },
    /// The app block's session with hud in its box (M35); `follower`:
    /// it's a follower credential, so hud is told who approves.
    Hud { session: &'a crate::labs::apps::hud::Session, follower: bool },
}

/// Text without terminal colours, trimmed.
#[cfg(feature = "labs")]
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                for c in it.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

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

    #[cfg(feature = "labs")]
    fn by(actor: &str) -> ChantBy {
        ChantBy { actor: Some(actor.into()), ..Default::default() }
    }

    #[cfg(feature = "labs")]
    #[test]
    fn approving_runs_status_line() {
        let mut g = gate("delivery");
        // The plan read, and a signed gate: as status gave it, with who.
        g.command = Some("chant approve ship approve-ship --env prod --plan sha256:ab12 --sign".into());
        assert_eq!(
            chant_args(&g, &by("github:alice")).unwrap(),
            ["ship", "approve-ship", "--actor", "github:alice", "--env", "prod", "--plan", "sha256:ab12", "--sign"]
        );
        // Someone else's approval of a signed gate can't be sealed here.
        let relayed = ChantBy { relayed: true, relayed_by: Some("github:owner".into()), ..by("github:bob") };
        let e = chant_args(&g, &relayed).unwrap_err();
        assert!(e.contains("needs a signed approval"), "{e}");
        // Unsigned: theirs, carried by the owner.
        g.command = Some("chant approve ship approve-ship --plan sha256:ab12".into());
        assert_eq!(
            chant_args(&g, &relayed).unwrap(),
            ["ship", "approve-ship", "--actor", "github:bob", "--relayed-by", "github:owner", "--plan", "sha256:ab12"]
        );
        // Not relayed: no --relayed-by, whatever's known.
        let owner = ChantBy { relayed_by: Some("github:owner".into()), ..by("github:owner") };
        assert_eq!(
            chant_args(&g, &owner).unwrap(),
            ["ship", "approve-ship", "--actor", "github:owner", "--plan", "sha256:ab12"]
        );
        // A line for another gate (or none) is rebuilt.
        g.command = Some("chant approve other approve-ship --plan sha256:ab12".into());
        g.env = Some("prod".into());
        assert_eq!(chant_args(&g, &ChantBy::default()).unwrap(), ["ship", "approve-ship", "--env", "prod"]);
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

    #[cfg(feature = "labs")]
    #[test]
    fn answering_runs_points_answer() {
        let q = question();
        assert_eq!(
            answer_args(&q, "medium", &by("github:alice")).unwrap(),
            ["workspace", "points", "answer", "slice-tier-1724faf3af79", "--answer", "medium", "--by", "github:alice"]
        );
        // Someone else's, carried by the owner.
        let relayed = ChantBy { relayed: true, relayed_by: Some("github:owner".into()), ..by("github:bob") };
        assert_eq!(
            answer_args(&q, "small", &relayed).unwrap()[6..],
            ["--by", "github:bob", "--relayed-by", "github:owner"]
        );
        // Not one of its choices, or nobody to name.
        assert!(answer_args(&q, "huge", &by("github:alice")).unwrap_err().contains("small, medium, large"));
        assert!(answer_args(&q, "small", &ChantBy::default()).unwrap_err().contains("principals"));
        // A gate isn't answered.
        assert!(answer_args(&gate("delivery"), "yes", &by("github:alice")).is_err());
        assert_eq!(
            points_said(r#"{"verb":"answer","question":{"title":"Which builder tier (W-001): medium"}}"#).as_deref(),
            Some("Which builder tier (W-001): medium")
        );
        assert_eq!(
            points_error(r#"{"error":{"code":"quorum-not-met","message":"needs 2 people"}}"#),
            Some(("needs 2 people".into(), "quorum-not-met".into()))
        );
    }

    #[cfg(feature = "labs")]
    #[test]
    fn expiring() {
        let mut g = gate("delivery");
        assert_eq!(expire_args(&g), ["ship", "approve-ship", "--expire"]);
        g.env = Some("prod".into());
        assert_eq!(expire_args(&g), ["ship", "approve-ship", "--env", "prod", "--expire"]);
    }

    #[test]
    fn times_and_text() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_500));
        assert_eq!(rfc3339_ms("yesterday"), None);
        #[cfg(feature = "labs")]
        assert_eq!(plain("\x1b[32mGate \"g\" resolved\x1b[0m\n"), "Gate \"g\" resolved");
    }
}
