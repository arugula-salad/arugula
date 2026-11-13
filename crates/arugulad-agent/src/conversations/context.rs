//! What a transcript says about the session's context window (#397): the
//! model, how full the window is, and the compactions.
//!
//! Fed one transcript line at a time ([`Context::line`]), in file order, by
//! the converter; [`Context::summary`] is what the agent block shows.
//!
//! **Fill** is the last assistant entry's `usage`: `input_tokens` plus the
//! two cache counts, over the model's window. Right after a compaction it's
//! what the compaction kept (the boundary's `postTokens`) plus what every
//! request carries besides the conversation (the system prompt, the tools):
//! `postTokens` counts only the conversation, so on its own the bar would
//! dip far below what the next turn reports (#444). That part is learned
//! from the turn after an earlier compaction, else taken from the session's
//! first turn, and the fill is marked estimated until the next entry.
//!
//! **Window** (#444): no transcript line says it, and the model it names
//! carries no `[1m]` marker even when the session ran with one. So it's,
//! in order: the window Claude Code's engine reported (the mod's signals,
//! #396, or claude-agent-acp's `usage_update`), 1M for a marked model or
//! once the session held more than 200k, then Claude Code's documented
//! default for the model ([`window_for`]). A model whose 1M window is an
//! option the transcript can't show (Sonnet 4.x, Opus 4.6) has an unknown
//! window: the strip shows tokens, not a percentage it would have to guess.
//!
//! **Compactions** follow Podium's rule (#397): an automatic one below
//! [`OVERFLOW_AT`] of the window is just a compaction; it's an overflow at
//! that fill or more, or when the window had been exceeded. A manual one is
//! always plain. Two sessions of theirs compacted at 22% and 25% of a 1M
//! window after a quiet stretch and were reported as running out. While the
//! window is unknown, only an exceeded window makes one an overflow; a
//! mark is judged again once the window is known.
//!
//! **From inside the session** (#396, #450): the Claude Code mod's
//! signals ([`Context::signal`]) feed the same summary for a terminal pane,
//! with what a transcript doesn't say: the window as the engine has it
//! (not guessed from the model's name), the effort, a compaction as it
//! happens, and Claude Code's own cost. Its summary says `source: live`,
//! with the versions; a transcript's says `source: transcript`.

use serde_json::{Value, json};

/// An automatic compaction at this fill or more is an overflow.
pub const OVERFLOW_AT: f64 = 0.8;

/// The window of a model without a long context.
pub const DEFAULT_WINDOW: u64 = 200_000;
/// A long-context window.
pub const LONG_WINDOW: u64 = 1_000_000;

/// The most a session's first turn holds and still say what every request
/// carries (the system prompt, the tools) rather than a resumed history.
const FIRST_TURN_MAX: u64 = 100_000;

/// What Claude Code gives a model (its docs, "Extended context"): Fable,
/// Sonnet 5 and later, Haiku 5.5 and Opus 4.7 and later run with a 1M
/// window by default; Opus 4.6 and Sonnet 4.x have one only as their
/// `[1m]` variant; the rest have 200k.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Long {
    /// 1M unless `CLAUDE_CODE_DISABLE_1M_CONTEXT` holds it to 200k.
    Native,
    /// 1M or 200k: the `[1m]` variant is chosen, and a transcript doesn't
    /// record the choice.
    Optional,
    /// 200k.
    No,
}

/// What a model id (`claude-sonnet-5-5`, `claude-3-5-sonnet-20241022`,
/// a Bedrock id) says of its window, or nothing for one this can't read.
pub fn long_context(model: &str) -> Option<Long> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.split('[').next().unwrap_or("");
    let mut family = None;
    let mut version = vec![];
    for part in m.split(['-', '.', '@']) {
        if ["opus", "sonnet", "haiku", "fable"].contains(&part) {
            family.get_or_insert(part);
        } else if !part.is_empty() && part.len() <= 2 && part.chars().all(|c| c.is_ascii_digit()) {
            version.push(part.parse::<u32>().ok()?);
        }
    }
    let v = (*version.first()?, version.get(1).copied().unwrap_or(0));
    Some(match family? {
        "fable" => Long::Native,
        "opus" if v >= (4, 7) => Long::Native,
        "opus" if v == (4, 6) => Long::Optional,
        "opus" => Long::No,
        "sonnet" if v.0 >= 5 => Long::Native,
        "sonnet" if v.0 == 4 => Long::Optional,
        "sonnet" => Long::No,
        "haiku" if v >= (5, 5) => Long::Native,
        "haiku" if v.0 >= 5 => Long::Optional,
        _ => Long::No,
    })
}

/// `CLAUDE_CODE_DISABLE_1M_CONTEXT` is set, in the daemon's environment or
/// Claude Code's user settings (`env`): native 1M models then have 200k.
/// Read once.
fn long_disabled() -> bool {
    #[cfg(test)]
    return false;
    #[cfg(not(test))]
    {
        static OFF: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
            const KEY: &str = "CLAUDE_CODE_DISABLE_1M_CONTEXT";
            let on = |v: &str| !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "0" | "false" | "no" | "off");
            if std::env::var(KEY).is_ok_and(|v| on(&v)) {
                return true;
            }
            let dir = super::Dirs::from_env().claude;
            let Ok(text) = std::fs::read_to_string(dir.join("settings.json")) else {
                return false;
            };
            let v: Value = serde_json::from_str(&text).unwrap_or_default();
            match &v["env"][KEY] {
                Value::String(s) => on(s),
                Value::Bool(b) => *b,
                Value::Number(n) => n.as_u64() != Some(0),
                _ => false,
            }
        });
        *OFF
    }
}

/// The window, in tokens, of a model as the transcript names it, or
/// nothing when it can't be told.
///
/// 1M for a model id or alias marked `[1m]` (the `/model opus[1m]` form
/// Claude Code takes) and for the `-1m` variant ids, and once `seen` (the
/// most tokens the session has held, from usage or a compaction's
/// `preTokens`) passes 200k: no 200k window holds more than that. Else
/// Claude Code's default for the model ([`long_context`]); a model whose
/// long window is an unrecorded choice, or one this can't read, is unknown.
pub fn window_for(model: Option<&str>, seen: u64) -> Option<u64> {
    let m = model.unwrap_or("").to_ascii_lowercase();
    let marked = m.contains("[1m]") || m.ends_with("-1m") || m.contains("-1m-");
    if marked || seen > DEFAULT_WINDOW {
        return Some(LONG_WINDOW);
    }
    match long_context(&m)? {
        Long::Native if long_disabled() => Some(DEFAULT_WINDOW),
        Long::Native => Some(LONG_WINDOW),
        Long::No => Some(DEFAULT_WINDOW),
        Long::Optional => None,
    }
}

/// `claude-opus-5-5` as `Opus 5.5`; `claude-3-5-sonnet-20241022[1m]` as
/// `Sonnet 3.5`. What it can't read it gives back as it is.
pub fn short_model(id: &str) -> String {
    let id = id.trim().to_ascii_lowercase();
    let id = id.split('[').next().unwrap_or("");
    let id = id.strip_prefix("claude-").unwrap_or(id);
    let mut family = vec![];
    let mut version = vec![];
    for part in id.split('-') {
        let digits = part.chars().all(|c| c.is_ascii_digit());
        // A trailing date (`20241022`) isn't a version; neither is `1m`.
        if digits && part.len() <= 2 {
            version.push(part);
        } else if !digits && part != "1m" && part != "latest" {
            let mut c = part.chars();
            family.extend(c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()));
        }
    }
    if family.is_empty() {
        return id.to_owned();
    }
    let v = version.join(".");
    if v.is_empty() { family.join(" ") } else { format!("{} {v}", family.join(" ")) }
}

/// What kind of mark a compaction gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Compacted, and the window was not the reason (or isn't known to be).
    Compacted,
    /// The window was full, or had been exceeded.
    Overflow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// `auto`, `manual`, or `window` for an exceeded window nothing has
    /// compacted since.
    pub trigger: String,
    pub at_ms: u64,
    /// Tokens held before it, when the transcript says.
    pub before: Option<u64>,
    /// The window then, when it was known.
    pub window: Option<u64>,
    /// The API had refused a prompt for its length.
    pub exceeded: bool,
}

impl Mark {
    /// Its kind, with `window` for a mark made while the window was unknown.
    fn kind(&self, window: Option<u64>) -> Kind {
        if self.trigger == "manual" {
            return Kind::Compacted;
        }
        let full = match (self.before, self.window.or(window)) {
            (Some(b), Some(w)) => b as f64 / w as f64 >= OVERFLOW_AT,
            _ => false,
        };
        if self.exceeded || full { Kind::Overflow } else { Kind::Compacted }
    }
}

#[derive(Clone, Default)]
pub struct Context {
    model: Option<String>,
    /// The last assistant entry's fill, in tokens.
    used: Option<u64>,
    /// `used` is an estimate: a compaction's, until the next entry.
    estimated: bool,
    /// The most tokens it has held, to tell a 1M window from 200k.
    seen: u64,
    /// The session's first fill: about what every request carries.
    first: Option<u64>,
    /// What a request carried beyond what the last compaction kept, from
    /// the turn after it.
    overhead: Option<u64>,
    /// What the last compaction kept, until the next entry.
    kept: Option<u64>,
    marks: Vec<Mark>,
    /// The API refused a prompt for its length, and nothing compacted since.
    exceeded: Option<u64>,
    /// The window as Claude Code has it (#396): only the mod says.
    engine_window: Option<u64>,
    /// The effort of the last model request (#396): only the mod says.
    effort: Option<String>,
    /// The mod has spoken (#450): Claude Code's version, the mod's, and
    /// what Claude Code says the session has cost.
    live: Option<Live>,
}

#[derive(Clone, Default)]
struct Live {
    claude_version: Option<String>,
    mod_version: Option<String>,
    cost_usd: Option<f64>,
}

impl Context {
    /// The window and where it came from: the engine's when the mod (or
    /// the ACP adapter) said, else from the model and what it held.
    fn window_from(&self) -> (Option<u64>, Option<&'static str>) {
        if let Some(w) = self.engine_window {
            return (Some(w), Some("engine"));
        }
        let w = window_for(self.model.as_deref(), self.seen);
        let from = w.map(|w| if w == LONG_WINDOW && self.seen > DEFAULT_WINDOW { "usage" } else { "model" });
        (w, from)
    }

    fn window(&self) -> Option<u64> {
        self.window_from().0
    }

    /// The window as Claude Code's engine reported it, from outside the
    /// transcript: claude-agent-acp's `usage_update` `size` (#444).
    pub fn set_engine_window(&mut self, w: Option<u64>) {
        if let Some(w) = w.filter(|w| *w > 0) {
            self.engine_window = Some(w);
        }
    }

    /// A measured fill (an assistant entry, or the mod's `usage`).
    fn filled(&mut self, used: u64) {
        if let Some(kept) = self.kept.take()
            && used > kept
        {
            self.overhead = Some(used - kept);
        }
        self.first.get_or_insert(used);
        self.used = Some(used);
        self.estimated = false;
        self.seen = self.seen.max(used);
    }

    /// What the Claude Code mod says from inside the session (#396), `at`
    /// when it arrived. A subagent's own loop says nothing of the main
    /// window, as a sidechain's transcript lines don't.
    pub fn signal(&mut self, s: &arugula_proto::api::SessionSignal, at: u64) {
        if s.agent_id.is_some() {
            return;
        }
        let live = self.live.get_or_insert_default();
        let text = |v: &Option<String>| v.as_deref().filter(|v| !v.is_empty()).map(str::to_owned);
        live.claude_version = text(&s.claude_version).or(live.claude_version.take());
        live.mod_version = text(&s.mod_version).or(live.mod_version.take());
        if let Some(c) = s.cost_usd.filter(|c| c.is_finite() && *c >= 0.0) {
            live.cost_usd = Some(c);
        }
        if let Some(m) = s.model.as_deref().filter(|m| !m.is_empty()) {
            self.model = Some(m.to_owned());
        }
        if let Some(e) = s.effort.as_deref().filter(|e| !e.is_empty()) {
            self.effort = Some(e.to_owned());
        }
        self.set_engine_window(s.window);
        match s.event.as_str() {
            "usage" => {
                if let Some(t) = s.tokens.filter(|t| *t > 0) {
                    self.filled(t);
                }
            }
            "compact" => {
                let trigger = s.trigger.clone().unwrap_or_else(|| "auto".into());
                let meta = json!({ "trigger": trigger, "preTokens": s.tokens_before, "postTokens": s.tokens_after });
                self.compacted(&json!({ "compactMetadata": meta }), at);
            }
            _ => {}
        }
    }

    pub fn line(&mut self, v: &Value) {
        if v["isSidechain"] == true {
            return;
        }
        let at = super::convert::at_ms(v["timestamp"].as_str().unwrap_or(""));
        match v["type"].as_str() {
            Some("assistant") => self.assistant(v, at),
            Some("system") if v["subtype"] == "compact_boundary" => self.compacted(v, at),
            _ => {}
        }
    }

    fn assistant(&mut self, v: &Value, at: u64) {
        let m = &v["message"];
        if v["isApiErrorMessage"] == true {
            let text: String = m["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let text = text.to_ascii_lowercase();
            if text.contains("prompt is too long") || text.contains("context window") || text.contains("context length")
            {
                self.exceeded = Some(at);
            }
            return;
        }
        if let Some(model) = m["model"].as_str().filter(|s| !s.is_empty() && !s.starts_with('<')) {
            self.model = Some(model.to_owned());
        }
        let u = &m["usage"];
        if u.is_object() {
            let n = |k: &str| u[k].as_u64().unwrap_or(0);
            let used = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
            // An entry without a count (a synthetic one) says nothing.
            if used > 0 {
                self.filled(used);
            }
        }
    }

    fn compacted(&mut self, v: &Value, at: u64) {
        let meta = &v["compactMetadata"];
        let trigger = meta["trigger"].as_str().unwrap_or("auto").to_owned();
        let before = meta["preTokens"].as_u64().or(self.used);
        self.seen = self.seen.max(before.unwrap_or(0));
        let window = self.window();
        self.marks.push(Mark { trigger, at_ms: at, before, window, exceeded: self.exceeded.is_some() });
        self.exceeded = None;
        // What it holds now is what the compaction kept, and what every
        // request carries besides (#444): the next entry says exactly.
        let kept = meta["postTokens"].as_u64();
        self.kept = kept;
        let besides = self.overhead.or(self.first.filter(|f| *f <= FIRST_TURN_MAX));
        self.used = kept.map(|k| k + besides.unwrap_or(0));
        self.estimated = self.used.is_some();
    }

    /// The marks so far; an exceeded window nothing has compacted since is
    /// the last of them.
    fn all_marks(&self) -> Vec<Mark> {
        let mut marks = self.marks.clone();
        if let Some(at) = self.exceeded {
            let window = self.window();
            marks.push(Mark { trigger: "window".into(), at_ms: at, before: self.used, window, exceeded: true });
        }
        marks
    }

    /// For the agent's state, or nothing when the transcript has said
    /// nothing about any of it.
    pub fn summary(&self) -> Option<Value> {
        let marks = self.all_marks();
        if self.model.is_none() && self.used.is_none() && marks.is_empty() {
            return None;
        }
        let (window, from) = self.window_from();
        let kind = |m: &Mark| m.kind(window);
        let last = marks.last().map(|m| {
            json!({
                "kind": match kind(m) { Kind::Compacted => "compacted", Kind::Overflow => "overflow" },
                "trigger": m.trigger,
                "at_ms": m.at_ms,
                "tokens_before": m.before,
                "window_before": m.window.or(window),
            })
        });
        let mut summary = json!({
            "model": self.model.as_deref().map(short_model),
            "tokens": self.used,
            "estimated": self.estimated,
            "window": window,
            "window_from": from,
            "compactions": marks.len(),
            "overflows": marks.iter().filter(|m| kind(m) == Kind::Overflow).count(),
            "last": last,
        });
        // Only the mod says (#396, #450).
        if let Some(e) = &self.effort {
            summary["effort"] = json!(e);
        }
        match &self.live {
            Some(l) => {
                summary["source"] = json!("live");
                summary["claude_version"] = json!(l.claude_version);
                summary["mod_version"] = json!(l.mod_version);
                summary["cost_usd"] = json!(l.cost_usd);
            }
            None => summary["source"] = json!("transcript"),
        }
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(model: &str, input: u64, read: u64, made: u64) -> String {
        json!({"type": "assistant", "timestamp": "2026-10-01T10:00:00.000Z",
            "message": {"model": model, "usage": {"input_tokens": input, "cache_read_input_tokens": read,
                "cache_creation_input_tokens": made, "output_tokens": 50}}})
        .to_string()
    }

    fn boundary(trigger: &str, pre: u64, post: u64) -> String {
        json!({"type": "system", "subtype": "compact_boundary", "timestamp": "2026-10-01T11:00:00.000Z",
            "compactMetadata": {"trigger": trigger, "preTokens": pre, "postTokens": post}})
        .to_string()
    }

    fn too_long() -> String {
        json!({"type": "assistant", "isApiErrorMessage": true, "timestamp": "2026-10-01T10:30:00.000Z",
            "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Prompt is too long"}],
                "usage": {"input_tokens": 0, "output_tokens": 0}}})
        .to_string()
    }

    fn read(lines: &[String]) -> Value {
        let mut c = Context::default();
        for l in lines {
            c.line(&serde_json::from_str(l).unwrap());
        }
        c.summary().expect("a summary")
    }

    #[test]
    fn normal_fill_is_the_last_entrys_three_counts_over_the_window() {
        let s =
            read(&[assistant("claude-opus-4-5", 1, 90_000, 9_999), assistant("claude-opus-4-5", 2, 100_000, 20_000)]);
        assert_eq!(s["tokens"], 120_002);
        assert_eq!(s["window"], 200_000);
        assert_eq!(s["window_from"], "model");
        assert_eq!(s["source"], "transcript");
        assert!(s.get("cost_usd").is_none());
        assert_eq!(s["model"], "Opus 4.5");
        assert_eq!(s["compactions"], 0);
        assert_eq!(s["estimated"], false);
        assert!(s["last"].is_null());
        // Opus 5.5 runs with 1M in Claude Code, marked or not.
        let s = read(&[assistant("claude-opus-5-5", 2, 100_000, 20_000)]);
        assert_eq!(s["window"], 1_000_000);
        assert_eq!(s["model"], "Opus 5.5");
    }

    #[test]
    fn near_full_is_still_just_fill() {
        let s = read(&[assistant("claude-sonnet-5-5", 3, 170_000, 1_000)]);
        assert_eq!(s["tokens"], 171_003);
        assert_eq!(s["compactions"], 0);
    }

    #[test]
    fn an_auto_compaction_at_low_fill_is_not_an_overflow() {
        // Podium's #803: 22% of a 1M window.
        let s = read(&[assistant("claude-opus-5-5[1m]", 1, 219_000, 0), boundary("auto", 220_000, 18_000)]);
        assert_eq!(s["window"], 1_000_000);
        assert_eq!(s["last"]["kind"], "compacted");
        assert_eq!(s["last"]["trigger"], "auto");
        assert_eq!(s["last"]["tokens_before"], 220_000);
        assert_eq!(s["overflows"], 0);
        // The fill is what it kept, until the next entry (the first turn
        // here is too big to say what every request carries).
        assert_eq!(s["tokens"], 18_000);
        assert_eq!(s["estimated"], true);
    }

    #[test]
    fn an_auto_compaction_at_80_percent_is_an_overflow() {
        let s = read(&[assistant("claude-opus-4-5", 1, 160_000, 0), boundary("auto", 160_000, 9_000)]);
        assert_eq!(s["last"]["kind"], "overflow");
        assert_eq!(s["last"]["window_before"], 200_000);
        // One below it isn't.
        let s = read(&[assistant("claude-opus-4-5", 1, 159_998, 0), boundary("auto", 159_999, 9_000)]);
        assert_eq!(s["last"]["kind"], "compacted");
    }

    #[test]
    fn a_manual_compaction_is_always_plain() {
        let s = read(&[assistant("claude-opus-5-5", 1, 199_000, 0), boundary("manual", 199_000, 9_000)]);
        assert_eq!(s["last"]["kind"], "compacted");
        assert_eq!(s["last"]["trigger"], "manual");
    }

    #[test]
    fn a_window_exceeded_stop_is_an_overflow_even_if_compacted_low() {
        // Refused for length, then compacted at a count that looks low.
        let s = read(&[assistant("claude-opus-5-5[1m]", 1, 300_000, 0), too_long(), boundary("auto", 300_000, 9_000)]);
        assert_eq!(s["last"]["kind"], "overflow");
        assert_eq!(s["compactions"], 1);
        // Not compacted since: the stop is the mark.
        let s = read(&[assistant("claude-opus-5-5", 1, 150_000, 0), too_long()]);
        assert_eq!(s["last"]["kind"], "overflow");
        assert_eq!(s["last"]["trigger"], "window");
        assert_eq!(s["tokens"], 150_001);
    }

    #[test]
    fn a_1m_model_has_a_1m_window() {
        assert_eq!(window_for(Some("opus[1m]"), 0), Some(1_000_000));
        assert_eq!(window_for(Some("claude-sonnet-4-5-1m"), 0), Some(1_000_000));
        assert_eq!(window_for(Some("claude-opus-5-5"), 0), Some(1_000_000));
        assert_eq!(window_for(Some("claude-opus-4-5"), 0), Some(200_000));
        assert_eq!(window_for(Some("claude-sonnet-4-6"), 0), None);
        assert_eq!(window_for(None, 0), None);
        // A 200k window can't have held more.
        assert_eq!(window_for(Some("claude-sonnet-4-6"), 967_206), Some(1_000_000));
        let s = read(&[assistant("claude-opus-4-6[1m]", 5, 400_000, 0)]);
        assert_eq!(s["window"], 1_000_000);
    }

    #[test]
    fn claude_codes_defaults_by_model() {
        use Long::*;
        for (id, long) in [
            ("claude-opus-5-5", Some(Native)),
            ("claude-opus-4-7", Some(Native)),
            ("claude-opus-4-6", Some(Optional)),
            ("claude-opus-4-5-20251101", Some(No)),
            ("claude-opus-4-20250514", Some(No)),
            ("claude-sonnet-5-5", Some(Native)),
            ("claude-sonnet-5", Some(Native)),
            ("claude-sonnet-4-6", Some(Optional)),
            ("claude-sonnet-4-5-20250929", Some(Optional)),
            ("us.anthropic.claude-sonnet-4-5-20250929-v1:0", Some(Optional)),
            ("claude-3-5-sonnet-20241022", Some(No)),
            ("claude-haiku-5-5", Some(Native)),
            ("claude-haiku-4-5-20251001", Some(No)),
            ("claude-fable-5-1", Some(Native)),
            ("opus", None),
            ("gpt-5", None),
        ] {
            assert_eq!(long_context(id), long, "{id}");
        }
    }

    #[test]
    fn a_1m_sonnet_5_5_session_compacts_without_overflowing() {
        // #444: Claude Code 2.1.316, Sonnet 5.5 with 1M and no marker
        // anywhere in the transcript, compacted automatically at ~190k.
        let s = read(&[
            assistant("claude-sonnet-5-5", 2, 40_000, 2_000),
            assistant("claude-sonnet-5-5", 2, 188_000, 1_500),
            boundary("auto", 189_502, 13_000),
            assistant("claude-sonnet-5-5", 2, 50_000, 2_000),
        ]);
        assert_eq!(s["window"], 1_000_000);
        assert_eq!(s["window_from"], "model");
        assert_eq!(s["model"], "Sonnet 5.5");
        assert_eq!(s["last"]["kind"], "compacted");
        assert_eq!(s["last"]["window_before"], 1_000_000);
        assert_eq!(s["overflows"], 0);
        assert_eq!(s["tokens"], 52_002);
    }

    #[test]
    fn an_unknown_window_says_tokens_and_calls_nothing_an_overflow() {
        // Sonnet 4.6 has 1M only as its [1m] variant, which the transcript
        // doesn't record: 170k could be 85% or 17%.
        let lines = [assistant("claude-sonnet-4-6", 1, 170_000, 0), boundary("auto", 170_001, 12_000)];
        let s = read(&lines);
        assert!(s["window"].is_null());
        assert!(s["window_from"].is_null());
        assert_eq!(s["last"]["kind"], "compacted");
        assert!(s["last"]["window_before"].is_null());
        assert_eq!(s["overflows"], 0);
        // An exceeded window is an overflow whatever the window.
        let s = read(&[assistant("claude-sonnet-4-6", 1, 120_000, 0), too_long()]);
        assert_eq!(s["last"]["kind"], "overflow");
        assert_eq!(s["tokens"], 120_001);
        // Once the engine says 200k, the same compaction was an overflow.
        let mut c = Context::default();
        for l in &lines {
            c.line(&serde_json::from_str(l).unwrap());
        }
        c.set_engine_window(Some(200_000));
        let s = c.summary().unwrap();
        assert_eq!(s["window"], 200_000);
        assert_eq!(s["window_from"], "engine");
        assert_eq!(s["last"]["kind"], "overflow");
        assert_eq!(s["last"]["window_before"], 200_000);
        // Once it has held more than 200k, it's 1M: a compaction then.
        let s = read(&[lines[0].clone(), lines[1].clone(), assistant("claude-sonnet-4-6", 1, 250_000, 0)]);
        assert_eq!(s["window"], 1_000_000);
        assert_eq!(s["window_from"], "usage");
        assert_eq!(s["last"]["kind"], "compacted");
    }

    #[test]
    fn the_fill_after_a_compaction_doesnt_dip() {
        // #444: a compaction's postTokens is the conversation alone; the
        // next turn also carries the system prompt and the tools.
        let first = assistant("claude-opus-5-5", 3, 40_000, 5_000);
        let s = read(&[first.clone(), assistant("claude-opus-5-5", 3, 330_000, 0), boundary("auto", 330_003, 13_000)]);
        // What it kept, plus what the first turn carried.
        assert_eq!(s["tokens"], 58_003);
        assert_eq!(s["estimated"], true);
        let s = read(&[
            first,
            assistant("claude-opus-5-5", 3, 330_000, 0),
            boundary("auto", 330_003, 13_000),
            assistant("claude-opus-5-5", 3, 50_000, 0),
            assistant("claude-opus-5-5", 3, 400_000, 0),
            boundary("auto", 400_003, 15_000),
        ]);
        // The turn after the first compaction says what it carried: 37,003.
        assert_eq!(s["tokens"], 52_003);
        assert_eq!(s["estimated"], true);
        assert_eq!(s["compactions"], 2);
    }

    #[test]
    fn models_are_shortened() {
        assert_eq!(short_model("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(short_model("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(short_model("claude-opus-5-5[1m]"), "Opus 5.5");
    }

    #[test]
    fn the_mods_window_settles_what_the_model_name_cant() {
        // #396: a 1M session whose model id carries no marker, compacted
        // automatically at 190k. From the name alone that's 95% of 200k.
        use arugula_proto::api::SessionSignal;
        let sig = |event: &str| SessionSignal { event: event.into(), ..Default::default() };
        let mut c = Context::default();
        c.signal(
            &SessionSignal { model: Some("claude-opus-5-5".into()), effort: Some("high".into()), ..sig("step") },
            1,
        );
        c.signal(&SessionSignal { tokens: Some(190_000), window: Some(1_000_000), ..sig("usage") }, 2);
        c.signal(
            &SessionSignal {
                trigger: Some("auto".into()),
                tokens_before: Some(190_000),
                tokens_after: Some(12_000),
                ..sig("compact")
            },
            3,
        );
        let s = c.summary().expect("a summary");
        assert_eq!(s["window"], 1_000_000);
        assert_eq!(s["window_from"], "engine");
        assert_eq!(s["source"], "live");
        assert_eq!(s["effort"], "high");
        assert_eq!(s["last"]["kind"], "compacted");
        assert_eq!(s["last"]["at_ms"], 3);
        assert_eq!(s["tokens"], 12_000);
        // A subagent's loop says nothing of the main window.
        c.signal(&SessionSignal { tokens: Some(5), agent_id: Some("a1".into()), ..sig("usage") }, 4);
        assert_eq!(c.summary().unwrap()["tokens"], 12_000);
        // Without the mod, Opus 5.5's window is Claude Code's default, 1M;
        // a model whose window the name can't settle calls it no overflow.
        let s = read(&[assistant("claude-opus-5-5", 1, 189_999, 0), boundary("auto", 190_000, 12_000)]);
        assert_eq!(s["last"]["kind"], "compacted");
        let s = read(&[assistant("claude-opus-4-6", 1, 189_999, 0), boundary("auto", 190_000, 12_000)]);
        assert_eq!(s["last"]["kind"], "compacted");
        assert!(s["window"].is_null());
    }

    #[test]
    fn the_mods_figures_say_they_are_live() {
        // #450: Claude Code's own cost and versions come with its signals,
        // and a later signal without them keeps what was said.
        use arugula_proto::api::SessionSignal;
        let mut c = Context::default();
        c.signal(
            &SessionSignal {
                event: "usage".into(),
                tokens: Some(52_000),
                window: Some(1_000_000),
                cost_usd: Some(1.25),
                claude_version: Some("2.1.316".into()),
                mod_version: Some("0.2.0".into()),
                ..Default::default()
            },
            1,
        );
        c.signal(
            &SessionSignal { event: "step".into(), model: Some("claude-sonnet-5-5".into()), ..Default::default() },
            2,
        );
        let s = c.summary().unwrap();
        assert_eq!(s["source"], "live");
        assert_eq!(s["cost_usd"], 1.25);
        assert_eq!(s["claude_version"], "2.1.316");
        assert_eq!(s["mod_version"], "0.2.0");
        assert_eq!(s["tokens"], 52_000);
        assert_eq!(s["model"], "Sonnet 5.5");
    }

    #[test]
    fn nothing_known_is_no_summary() {
        let mut c = Context::default();
        c.line(&json!({"type": "user"}));
        assert!(c.summary().is_none());
    }
}
