//! #304: an agent started from a chant workspace member (M34).
//!
//! Whatever starts an agent names its session, and whatever ran it writes
//! its run records (chant's ws-067, ws-075, ws-076). So an agent block
//! opened from a member card carries [`Chant`] in its config, and:
//!
//! - its agent server runs with `CHANT_AGENT` set to the agent session the
//!   declaration binds to the member, so chant judges its writes by that
//!   session's scope; Claude is also told, on its system prompt, which
//!   session it runs as.
//! - each turn is an agent run in the workspace's run ledger: `chant
//!   workspace runs start` when the prompt goes, `runs end` with the stop
//!   reason, tokens and cost when the turn ends. The block picks the run's
//!   id before the prompt goes, so the end needs nothing back from the
//!   start. The prompt is pinned by hash, never copied.
//! - #590: each prompt carries one more text block, [`turn_note`], naming
//!   the trailers the turn's commits end with: `Chant-Agent` (when the
//!   member has a session) and `Chant-Run` with the turn's run id, so
//!   chant's `graph --intent` joins a commit to the run that made it. Every
//!   agent gets it (Claude, Codex, any ACP agent), since it is part of the
//!   prompt; its `_meta` ([`RUN_META`]) keeps it out of the transcript.
//!
//! The writes go one at a time, in order, with the user's shell environment
//! (#74) on the block's host. A failure is a note in the transcript; the
//! turn goes on regardless.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use super::TurnStat;
use crate::{block::BlockCtx, review::Runner};

/// The environment variable naming the agent session a write is made in.
pub const AGENT_ENV: &str = "CHANT_AGENT";

/// The workspace member an agent was started from (in `layout.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chant {
    /// The workspace root, where the run ledger is.
    pub root: String,
    pub member: String,
    /// The agent session the declaration binds to the member, if it binds
    /// one: `CHANT_AGENT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The workspace's chant, as the workspace block found it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chant: Option<String>,
}

/// One write to the run ledger.
#[derive(Debug, Clone, PartialEq)]
pub enum Write {
    Start { id: String, fields: Value },
    End { id: String, fields: Value },
}

/// A turn's run id: the block and when its prompt went, so unique in the
/// ledger and the same if the block picks it again after a restart.
pub fn run_id(block: u32, started_ms: u64) -> String {
    format!("arugula-{block}-{started_ms}")
}

/// The `_meta` key on the prompt's text block that names the turn's run
/// (#590): the block is for the agent, so the transcript leaves it out.
pub const RUN_META: &str = "arugula/chantRun";

/// How [`turn_note`] starts: what a replayed transcript is cut at.
pub const NOTE_PREFIX: &str = "This turn is the chant agent run `";

/// Whether a prompt gets [`turn_note`]: not a slash command (`/usage`,
/// `/compact args`, a custom command), which Claude Code's adapter reads
/// from the first block alone, taking anything after it as the command's
/// arguments, and which `/usage` only runs as a prompt of one block.
pub fn takes_note(text: &str) -> bool {
    !text.trim_start().starts_with('/')
}

/// A replayed user message without the [`turn_note`] the agent was sent
/// with it (a session loaded again replays the prompt as the agent got it).
pub fn strip_notes(t: &mut super::transcript::Transcript) {
    for e in &mut t.entries {
        if let super::transcript::Entry::User { text, .. } = e
            && let Some(at) = text.find(NOTE_PREFIX)
        {
            text.truncate(at);
            text.truncate(text.trim_end().len());
        }
    }
}

/// What a prompt tells the agent about the turn's run (#590): the trailers
/// each commit it makes in the turn ends with.
pub fn turn_note(c: &Chant, run: &str) -> String {
    let mut trailers = String::new();
    if let Some(a) = &c.agent {
        trailers.push_str(&format!("Chant-Agent: {a}\n"));
    }
    trailers.push_str(&format!("Chant-Run: {run}"));
    format!(
        "{NOTE_PREFIX}{run}` in the workspace member `{}`. \
         End the message of every commit you make in this turn with these trailers, as the last lines:\n\n{trailers}",
        c.member
    )
}

/// The text block [`turn_note`] goes in.
pub fn turn_block(c: &Chant, run: &str) -> Value {
    json!({ "type": "text", "text": turn_note(c, run), "_meta": { RUN_META: run } })
}

/// The harness chant records for an agent kind.
pub fn harness(def: &super::defs::Def) -> String {
    use super::defs::Kind;
    match def.agent {
        Kind::Claude => "claude-code".into(),
        Kind::Codex => "codex".into(),
        Kind::Fountain => "fountain".into(),
        Kind::Acp => def.label(),
    }
}

/// The session's model, from its `configOptions` (ACP's `model` option).
pub fn model(config_options: &Value) -> Option<String> {
    config_options
        .as_array()?
        .iter()
        .find(|o| o["id"] == "model" || o["category"] == "model")
        .and_then(|o| o["currentValue"].as_str())
        .map(str::to_owned)
}

/// `runs start`'s fields for a turn.
pub fn start_fields(c: &Chant, id: &str, started_ms: u64, harness: &str, model: Option<&str>, prompt: &str) -> Value {
    let mut f = json!({
        "id": id,
        "startedAt": rfc3339(started_ms),
        "harness": harness,
        "instruction": { "sha256": hex::encode(Sha256::digest(prompt.as_bytes())) },
    });
    if let Some(a) = &c.agent {
        f["agent"] = json!(a);
    }
    if let Some(m) = model {
        f["model"] = json!(m);
    }
    f
}

/// `runs end`'s fields for a turn that ended: its stop reason, its tokens
/// (ACP's `usage`) and its cost, when the agent priced it.
pub fn end_fields(t: &TurnStat, currency: Option<&str>, harness: &str) -> Value {
    let mut f = json!({ "endedAt": rfc3339(t.ended_ms.unwrap_or(t.started_ms)) });
    if let Some(stop) = &t.stop {
        f["outcome"] = json!(stop);
    }
    let mut usage = json!({ "turns": 1 });
    if let Some(u) = &t.tokens {
        for (ours, theirs) in [
            ("inputTokens", "inputTokens"),
            ("outputTokens", "outputTokens"),
            ("cacheReadTokens", "cachedReadTokens"),
            ("cacheWriteTokens", "cachedWriteTokens"),
        ] {
            if let Some(n) = u[theirs].as_u64() {
                usage[ours] = json!(n);
            }
        }
    }
    f["usage"] = usage;
    // A cost with no ISO 4217 currency isn't one chant can total; it's
    // left out (unpriced), never sent as zero.
    if let (Some(amount), Some(cur)) =
        (t.cost, currency.filter(|c| c.len() == 3 && c.chars().all(|c| c.is_ascii_uppercase())))
        && amount >= 0.0
    {
        f["cost"] = json!({ "amount": amount, "currency": cur, "source": format!("harness:{harness}") });
    }
    f
}

/// What a Claude session is told, on its system prompt, about the session
/// it runs as.
pub fn system_prompt(c: &Chant) -> Option<String> {
    let a = c.agent.as_ref()?;
    Some(format!(
        "You are working in the chant workspace member `{}` as the agent session `{a}` (CHANT_AGENT is set). \
         End the message of every commit you make with the trailer `Chant-Agent: {a}`, and the `Chant-Run` \
         trailer the prompt names for its turn.",
        c.member
    ))
}

/// `sh -c WRITE sh ROOT CHANT FIELDS VERB [ID]`: one ledger write, chant's
/// document on stdout.
const WRITE: &str = r#"cd "$1" || { printf '{"error":{"message":"no such directory: %s"}}' "$1"; exit 0; }
c=$2 f=$3; shift 3
printf '%s' "$f" | "$c" workspace runs "$@" --from -"#;

/// Why a write failed, from chant's document (or its absence).
pub fn failure(out: &[u8], code: Option<i32>) -> Option<String> {
    let doc: Value = serde_json::from_slice(out).unwrap_or(Value::Null);
    if let Some(m) = doc["error"]["message"].as_str() {
        let code = doc["error"]["code"].as_str().map(|c| format!("{c}: ")).unwrap_or_default();
        return Some(format!("{code}{m}"));
    }
    if doc["run"].is_object() && code == Some(0) {
        return None;
    }
    Some(match code {
        Some(127) => "chant isn't there to run".into(),
        Some(c) => format!("chant exited {c} without saying why"),
        None => "chant was stopped".into(),
    })
}

/// The block's writer: writes in order, each reported to `done` as a note
/// for the block's log (`{"e":"run_recorded"}` or `{"e":"run_failed"}`).
pub fn writer(ctx: &BlockCtx, c: Chant, done: impl Fn(Value) + Send + 'static) -> mpsc::UnboundedSender<Write> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Write>();
    let ctx = ctx.clone();
    ctx.rt.clone().spawn(async move {
        let mut runner: Option<Result<Runner, String>> = None;
        while let Some(w) = rx.recv().await {
            if runner.is_none() {
                runner = Some(Runner::user(&ctx).await);
            }
            let (verb, id, fields) = match &w {
                Write::Start { id, fields } => ("start", id.clone(), fields),
                Write::End { id, fields } => ("end", id.clone(), fields),
            };
            let chant = c.chant.clone().unwrap_or_else(|| "chant".into());
            let mut args = vec![c.root.clone(), chant, fields.to_string(), verb.to_owned()];
            if verb == "end" {
                args.push(id.clone());
            }
            let why = match runner.as_ref().unwrap() {
                Err(e) => Some(e.clone()),
                Ok(r) => match r.sh(WRITE, &args).await {
                    Ok((out, code)) => failure(&out, code),
                    Err(e) => Some(e),
                },
            };
            done(match why {
                None => json!({ "e": "run_recorded", "verb": verb, "id": id }),
                Some(message) => json!({ "e": "run_failed", "verb": verb, "id": id, "message": message }),
            });
        }
    });
    tx
}

/// RFC 3339 in UTC, to the millisecond.
pub fn rfc3339(ms: u64) -> String {
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, s, milli) = (rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}.{milli:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(agent: Option<&str>) -> Chant {
        Chant { root: "/w".into(), member: "app".into(), agent: agent.map(str::to_owned), chant: None }
    }

    #[test]
    fn times_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(1_791_234_567_890), "2026-10-05T21:09:27.890Z");
    }

    #[test]
    fn a_start_names_the_session_and_pins_the_prompt() {
        let f = start_fields(&member(Some("app")), "arugula-7-1", 0, "claude-code", Some("opus"), "fix it");
        assert_eq!(f["id"], "arugula-7-1");
        assert_eq!(f["agent"], "app");
        assert_eq!(f["harness"], "claude-code");
        assert_eq!(f["model"], "opus");
        assert_eq!(f["startedAt"], "1970-01-01T00:00:00.000Z");
        let pin = f["instruction"]["sha256"].as_str().unwrap();
        assert_eq!(pin.len(), 64);
        // Nothing the person said is copied.
        assert!(!f.to_string().contains("fix it"));
        let f = start_fields(&member(None), "x", 0, "codex", None, "");
        assert!(f.get("agent").is_none() && f.get("model").is_none());
    }

    #[test]
    fn an_end_carries_outcome_tokens_and_cost() {
        let t = TurnStat {
            started_ms: 1000,
            ended_ms: Some(2000),
            stop: Some("end_turn".into()),
            cost: Some(0.25),
            tokens: Some(json!({ "totalTokens": 30, "inputTokens": 10, "outputTokens": 20, "cachedReadTokens": 5 })),
            ..Default::default()
        };
        let f = end_fields(&t, Some("USD"), "claude-code");
        assert_eq!(f["outcome"], "end_turn");
        assert_eq!(f["endedAt"], "1970-01-01T00:00:02.000Z");
        assert_eq!(f["usage"], json!({ "turns": 1, "inputTokens": 10, "outputTokens": 20, "cacheReadTokens": 5 }));
        assert_eq!(f["cost"], json!({ "amount": 0.25, "currency": "USD", "source": "harness:claude-code" }));
        // No currency, or not an ISO one: unpriced.
        assert!(end_fields(&t, None, "claude-code").get("cost").is_none());
        assert!(end_fields(&t, Some("usd"), "claude-code").get("cost").is_none());
    }

    #[test]
    fn the_model_comes_from_config_options() {
        let opts = json!([{ "id": "mode", "currentValue": "default" }, { "id": "model", "currentValue": "sonnet" }]);
        assert_eq!(model(&opts).as_deref(), Some("sonnet"));
        assert_eq!(model(&Value::Null), None);
    }

    #[test]
    fn failures_come_from_chant_s_document() {
        let ok = br#"{"verb":"start","run":{"id":"x"},"trailer":"Chant-Run: x"}"#;
        assert_eq!(failure(ok, Some(0)), None);
        let refused = br#"{"verb":"end","error":{"code":"run-unknown","message":"no run x"}}"#;
        assert_eq!(failure(refused, Some(1)).as_deref(), Some("run-unknown: no run x"));
        assert_eq!(failure(b"", Some(127)).as_deref(), Some("chant isn't there to run"));
    }

    #[test]
    fn slash_commands_go_alone_and_a_replay_drops_the_note() {
        assert!(takes_note("fix the port"));
        assert!(!takes_note("/usage") && !takes_note(" /compact keep the plan"));
        let mut t = super::super::transcript::Transcript::default();
        let note = turn_note(&member(Some("app")), "arugula-7-1");
        for chunk in ["fix it", "\n\n", &note[..10], &note[10..]] {
            t.apply(&json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": chunk } }), 0);
        }
        t.apply(&json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "done" } }), 0);
        t.apply(
            &json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": "no note here" } }),
            0,
        );
        strip_notes(&mut t);
        let users: Vec<_> = t
            .entries
            .iter()
            .filter_map(|e| match e {
                super::super::transcript::Entry::User { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, ["fix it", "no note here"]);
    }

    #[test]
    fn each_turn_names_its_run_trailer_and_the_session_s() {
        let b = turn_block(&member(Some("app")), "arugula-7-1");
        assert_eq!(b["_meta"][RUN_META], "arugula-7-1");
        let text = b["text"].as_str().unwrap();
        assert!(text.ends_with("\n\nChant-Agent: app\nChant-Run: arugula-7-1"), "{text}");
        assert!(text.contains("member `app`"), "{text}");
        // No session: only the run.
        let text = turn_note(&member(None), "arugula-7-1");
        assert!(text.ends_with("\n\nChant-Run: arugula-7-1") && !text.contains("Chant-Agent"), "{text}");
    }

    #[test]
    fn claude_is_told_its_session_only_when_there_is_one() {
        assert!(system_prompt(&member(Some("app"))).unwrap().contains("Chant-Agent: app"));
        assert_eq!(system_prompt(&member(None)), None);
    }
}
