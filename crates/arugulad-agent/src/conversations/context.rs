//! What a transcript says about the session's context window (#397): the
//! model, how full the window is, and the compactions.
//!
//! Fed one transcript line at a time ([`Context::line`]), in file order, by
//! the converter; [`Context::summary`] is what the agent block shows.
//!
//! **Fill** is the last assistant entry's `usage`: `input_tokens` plus the
//! two cache counts, over the model's window. A compaction empties it until
//! the next assistant entry (the boundary's `postTokens`, if it has them).
//!
//! **Compactions** follow Podium's rule (#397): an automatic one below
//! [`OVERFLOW_AT`] of the window is just a compaction; it's an overflow at
//! that fill or more, or when the window had been exceeded. A manual one is
//! always plain. Two sessions of theirs compacted at 22% and 25% of a 1M
//! window after a quiet stretch and were reported as running out.

use serde_json::{Value, json};

/// An automatic compaction at this fill or more is an overflow.
pub const OVERFLOW_AT: f64 = 0.8;

/// The window, in tokens, of a model as the transcript names it.
///
/// Sizes are Anthropic's published context windows: 200k by default, 1M for
/// a model id or alias marked `[1m]` (the `/model opus[1m]` form Claude Code
/// takes) and for the `-1m` variant ids. Transcripts usually record the
/// model without the marker, so `seen` (the most tokens the session has
/// held, from usage or a compaction's `preTokens`) lifts a 200k answer: no
/// 200k window holds more than that.
pub fn window_for(model: Option<&str>, seen: u64) -> u64 {
    const DEFAULT: u64 = 200_000;
    const LONG: u64 = 1_000_000;
    let m = model.unwrap_or("").to_ascii_lowercase();
    let marked = m.contains("[1m]") || m.ends_with("-1m") || m.contains("-1m-");
    if marked || seen > DEFAULT { LONG } else { DEFAULT }
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
    /// Compacted, and the window was not the reason.
    Compacted,
    /// The window was full, or had been exceeded.
    Overflow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub kind: Kind,
    /// `auto`, `manual`, or `window` for an exceeded window nothing has
    /// compacted since.
    pub trigger: String,
    pub at_ms: u64,
    /// Tokens held before it, when the transcript says.
    pub before: Option<u64>,
    /// The window then.
    pub window: u64,
}

#[derive(Clone, Default)]
pub struct Context {
    model: Option<String>,
    /// The last assistant entry's fill, in tokens.
    used: Option<u64>,
    /// The most tokens it has held, to tell a 1M window from 200k.
    seen: u64,
    marks: Vec<Mark>,
    /// The API refused a prompt for its length, and nothing compacted since.
    exceeded: Option<u64>,
}

impl Context {
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
                self.used = Some(used);
                self.seen = self.seen.max(used);
            }
        }
    }

    fn compacted(&mut self, v: &Value, at: u64) {
        let meta = &v["compactMetadata"];
        let trigger = meta["trigger"].as_str().unwrap_or("auto").to_owned();
        let before = meta["preTokens"].as_u64().or(self.used);
        self.seen = self.seen.max(before.unwrap_or(0));
        let window = window_for(self.model.as_deref(), self.seen);
        let full = before.is_some_and(|b| b as f64 / window as f64 >= OVERFLOW_AT);
        let overflow = trigger != "manual" && (full || self.exceeded.is_some());
        self.marks.push(Mark {
            kind: if overflow { Kind::Overflow } else { Kind::Compacted },
            trigger,
            at_ms: at,
            before,
            window,
        });
        self.exceeded = None;
        // What it holds now is what the compaction kept.
        self.used = meta["postTokens"].as_u64();
    }

    /// The marks so far; an exceeded window nothing has compacted since is
    /// the last of them.
    fn all_marks(&self) -> Vec<Mark> {
        let mut marks = self.marks.clone();
        if let Some(at) = self.exceeded {
            let window = window_for(self.model.as_deref(), self.seen);
            marks.push(Mark { kind: Kind::Overflow, trigger: "window".into(), at_ms: at, before: self.used, window });
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
        let window = window_for(self.model.as_deref(), self.seen);
        let last = marks.last().map(|m| {
            json!({
                "kind": match m.kind { Kind::Compacted => "compacted", Kind::Overflow => "overflow" },
                "trigger": m.trigger,
                "at_ms": m.at_ms,
                "tokens_before": m.before,
                "window_before": m.window,
            })
        });
        Some(json!({
            "model": self.model.as_deref().map(short_model),
            "tokens": self.used,
            "window": window,
            "compactions": marks.len(),
            "overflows": marks.iter().filter(|m| m.kind == Kind::Overflow).count(),
            "last": last,
        }))
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
    fn normal_fill_is_the_last_entrys_three_counts_over_200k() {
        let s =
            read(&[assistant("claude-opus-5-5", 1, 90_000, 9_999), assistant("claude-opus-5-5", 2, 100_000, 20_000)]);
        assert_eq!(s["tokens"], 120_002);
        assert_eq!(s["window"], 200_000);
        assert_eq!(s["model"], "Opus 5.5");
        assert_eq!(s["compactions"], 0);
        assert!(s["last"].is_null());
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
        // The fill is what it kept, until the next entry.
        assert_eq!(s["tokens"], 18_000);
    }

    #[test]
    fn an_auto_compaction_at_80_percent_is_an_overflow() {
        let s = read(&[assistant("claude-opus-5-5", 1, 160_000, 0), boundary("auto", 160_000, 9_000)]);
        assert_eq!(s["last"]["kind"], "overflow");
        // One below it isn't.
        let s = read(&[assistant("claude-opus-5-5", 1, 159_998, 0), boundary("auto", 159_999, 9_000)]);
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
        assert_eq!(window_for(Some("opus[1m]"), 0), 1_000_000);
        assert_eq!(window_for(Some("claude-sonnet-5-5-1m"), 0), 1_000_000);
        assert_eq!(window_for(Some("claude-opus-5-5"), 0), 200_000);
        assert_eq!(window_for(None, 0), 200_000);
        // A 200k window can't have held more.
        assert_eq!(window_for(Some("claude-opus-5-5"), 967_206), 1_000_000);
        let s = read(&[assistant("claude-opus-5-5[1m]", 5, 400_000, 0)]);
        assert_eq!(s["window"], 1_000_000);
    }

    #[test]
    fn models_are_shortened() {
        assert_eq!(short_model("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(short_model("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(short_model("claude-opus-5-5[1m]"), "Opus 5.5");
    }

    #[test]
    fn nothing_known_is_no_summary() {
        let mut c = Context::default();
        c.line(&json!({"type": "user"}));
        assert!(c.summary().is_none());
    }
}
