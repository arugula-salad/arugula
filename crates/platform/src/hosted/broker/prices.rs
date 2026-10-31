//! What a Claude call costs at Anthropic's list prices, and how its usage
//! is read from the answer as it streams by: studio's lobby's
//! `lobby/metering.mjs` (studio-018, studio-058), ported, so a cap counts
//! the same dollars on the platform as it did in the lobby.
//!
//! - The table is `arugula-prices`' ([`arugula_prices::PRICES`]), the one
//!   the daemon's cost estimates use too (#398): USD per million tokens,
//!   keyed by model id prefix, most specific first, cache writes at 1.25×
//!   and 2× input, a row's rates for a long prompt, fast mode doubling the
//!   models that have it.
//! - A model the table doesn't name is priced as the dearest row, and
//!   logged once, so a new model is only ever over-counted, never given
//!   away.

use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};

pub use arugula_prices::{FAST, Price, Usage, dearest};
use serde_json::Value;
use tracing::info;

/// The price row for a model id, and whether the table named it. An
/// unknown id is priced as the dearest row, and logged the first time it's
/// seen.
pub fn price_of(model: &str) -> (Price, bool) {
    if let Some(p) = arugula_prices::lookup(model) {
        return (p, true);
    }
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if seen.lock().map(|mut s| s.insert(model.to_owned())).unwrap_or(false) {
        let d = dearest();
        info!(
            model = if model.is_empty() { "a response with no model" } else { model },
            priced_as = d.prefix,
            "a model isn't in the price table; priced as the dearest until it names it"
        );
    }
    (dearest(), false)
}

/// USD for one response of `model` that used `usage`, and the row it was
/// priced at.
pub fn cost_of(model: &str, usage: &Usage) -> (f64, &'static str) {
    let (row, _) = price_of(model);
    (arugula_prices::cost(&row, usage), row.prefix)
}

/// What a Messages request is reserved at before it goes out, in
/// millionths of a dollar: as if every byte of its body below
/// `max_tokens` were a third of a token of input written to the one-hour
/// cache (the dearest input there is), and it used all of `max_tokens`,
/// at `model`'s dearest rates (a long prompt's, for a row that has them;
/// the dearest row for a model the table doesn't name), doubled in fast
/// mode. With no `max_tokens` (Anthropic refuses those), `NO_MAX` of them.
pub fn reservation(model: &str, body_bytes: usize, max_tokens: Option<u64>, fast: bool) -> u64 {
    const NO_MAX: u64 = 128_000;
    let (row, _) = price_of(model);
    let (input_usd, output_usd) = row.dearest_rates();
    let input = (body_bytes as u64).div_ceil(3) as f64;
    let output = max_tokens.unwrap_or(NO_MAX) as f64;
    let times = if fast && FAST.contains(&row.prefix) { 2.0 } else { 1.0 };
    micros((input * input_usd * 2.0 + output * output_usd) * times / 1e6)
}

/// Dollars as the ledger keeps them: millionths, rounded.
pub fn micros(usd: f64) -> u64 {
    if usd.is_finite() && usd > 0.0 { (usd * 1e6).round() as u64 } else { 0 }
}

/// Reads a Messages API response as it streams by, JSON or server-sent
/// events, and says what it used: [`Reader::feed`] each chunk, then
/// [`Reader::result`]. For events, `message_start` carries the model and
/// the input side, and each `message_delta` the running output side; the
/// last of each wins. Events are read line by line as they come, so a long
/// stream is never held whole; a JSON body is (it's one message).
#[derive(Debug, Default)]
pub struct Reader {
    text: String,
    model: Option<String>,
    usage: Option<serde_json::Map<String, Value>>,
    /// The final usage arrived: a `message_delta`'s, or a whole JSON
    /// message's.
    last: bool,
}

impl Reader {
    fn take(&mut self, json: &Value) {
        let message = match json.get("type").and_then(Value::as_str) {
            Some("message_start") => json.get("message"),
            Some("message") => Some(json),
            Some("message_delta") => {
                if let Some(u) = json.get("usage").and_then(Value::as_object) {
                    self.last = true;
                    let usage = self.usage.get_or_insert_with(Default::default);
                    for (k, v) in u {
                        if !v.is_null() {
                            usage.insert(k.clone(), v.clone());
                        }
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(m) = message {
            if let Some(model) = m.get("model").and_then(Value::as_str) {
                self.model = Some(model.to_owned());
            }
            if let Some(u) = m.get("usage").and_then(Value::as_object) {
                let usage = self.usage.get_or_insert_with(Default::default);
                for (k, v) in u {
                    usage.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        self.text.push_str(&String::from_utf8_lossy(chunk));
        if self.text.starts_with('{') {
            return;
        }
        let Some(end) = self.text.rfind('\n') else { return };
        let rest = self.text.split_off(end + 1);
        let lines = std::mem::replace(&mut self.text, rest);
        for line in lines.lines() {
            if let Some(data) = line.strip_prefix("data:")
                && let Ok(json) = serde_json::from_str::<Value>(data.trim())
            {
                self.take(&json);
            }
        }
    }

    /// The model and what it used, or nothing when the response held no
    /// usage; and whether that's its final usage (a stream's
    /// `message_delta`, or a whole JSON message), which is all a call is
    /// settled at.
    pub fn result(mut self) -> Option<(String, Usage, bool)> {
        let text = std::mem::take(&mut self.text);
        if text.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<Value>(text.trim()) {
                self.last = json.get("type").and_then(Value::as_str) == Some("message") && json.get("usage").is_some();
                self.take(&json);
            }
        } else if let Some(json) = text.strip_prefix("data:").and_then(|b| serde_json::from_str::<Value>(b.trim()).ok())
        {
            self.take(&json);
        }
        let usage = self.usage?;
        Some((self.model.unwrap_or_default(), Usage::from_json(&Value::Object(usage)), self.last))
    }
}

/// Milliseconds since the epoch at the start of `ms`'s UTC month.
pub fn month_start(ms: u64) -> u64 {
    let (y, m, _) = civil(ms / 86_400_000);
    days_from_civil(y, m, 1) * 86_400_000
}

/// The start of the next UTC month.
pub fn next_month(ms: u64) -> u64 {
    let (y, m, _) = civil(ms / 86_400_000);
    let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(y, m, 1) * 86_400_000
}

/// `2026-10`: the month `ms` is in, UTC.
pub fn month_of(ms: u64) -> String {
    let (y, m, _) = civil(ms / 86_400_000);
    format!("{y:04}-{m:02}")
}

/// `2026-11-01`: the day `ms` is, UTC.
pub fn day_of(ms: u64) -> String {
    let (y, m, d) = civil(ms / 86_400_000);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The start of month `2026-10` (UTC), or nothing for anything else.
pub fn parse_month(s: &str) -> Option<u64> {
    let (y, m) = s.split_once('-')?;
    if y.len() != 4 || m.len() != 2 {
        return None;
    }
    let (y, m): (u64, u64) = (y.parse().ok()?, m.parse().ok()?);
    ((1970..=9999).contains(&y) && (1..=12).contains(&m)).then(|| days_from_civil(y, m, 1) * 86_400_000)
}

/// Howard Hinnant's `civil_from_days`, for days since 1970-01-01.
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

fn days_from_civil(y: u64, m: u64, d: u64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usd(model: &str, u: Value) -> f64 {
        cost_of(model, &Usage::from_json(&u)).0
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The broker's sums on the shared table: fast mode doubling only the
    /// models that have it, and an unknown model at the dearest row. (The
    /// table's own prices are `arugula-prices`' tests.)
    #[test]
    fn prices_are_the_tables() {
        assert!(close(usd("claude-opus-5-20261001", json!({ "input_tokens": 1_000_000 })), 5.0));
        assert!(close(usd("claude-opus-5-5", json!({ "input_tokens": 1_000_000 })), 4.0), "most specific first");
        assert!(close(usd("claude-haiku-4-5", json!({ "cache_creation_input_tokens": 1_000_000 })), 1.25));
        let fast = json!({ "output_tokens": 1_000_000, "speed": "fast" });
        assert!(close(usd("claude-opus-5-5", fast.clone()), 40.0));
        assert!(close(usd("claude-opus-4-7", fast), 25.0), "no fast mode, no double");
        let (p, known) = price_of("claude-next-9");
        assert!(!known);
        assert_eq!(p.prefix, "claude-fable-5-1");
        assert!(close(usd("claude-next-9", json!({ "output_tokens": 1_000_000 })), 50.0));
        let search = json!({ "server_tool_use": { "web_search_requests": 3 } });
        assert!(close(usd("claude-haiku-4-5", search), 0.03), "web searches at $10 a thousand");
        // A reservation: 3,000 bytes as 1,000 tokens of one-hour cache
        // writes, and all of max_tokens.
        assert_eq!(
            reservation("claude-opus-5", 3000, Some(1000), false),
            micros((1000.0 * 10.0 + 1000.0 * 25.0) / 1e6)
        );
        assert_eq!(reservation("claude-opus-5-5", 0, Some(1000), true), micros(1000.0 * 20.0 * 2.0 / 1e6));
        assert_eq!(reservation("claude-next", 0, None, false), micros(128_000.0 * 50.0 / 1e6), "the dearest row");
        // A row with a long prompt's rates reserves at them.
        assert_eq!(reservation("claude-haiku-5-5", 0, Some(1000), false), micros(1000.0 * 2.5 / 1e6));
        assert_eq!(micros(1.234_567_8), 1_234_568);
        assert_eq!(micros(f64::NAN), 0);
    }

    /// Server-sent events split anywhere, a JSON body, and a body with no
    /// usage.
    #[test]
    fn usage_is_read_as_it_streams() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":4,\"output_tokens\":1}}}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7,\"input_tokens\":null}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":42}}\n\n",
        );
        for cut in [1, 7, 50, 120, 200, sse.len() - 3] {
            let mut r = Reader::default();
            r.feed(&sse.as_bytes()[..cut]);
            r.feed(&sse.as_bytes()[cut..]);
            let (model, u, last) = r.result().unwrap();
            assert_eq!(model, "claude-opus-5");
            assert_eq!((u.input, u.output, u.cache_read, last), (10, 42, 4, true), "cut at {cut}");
        }
        // Cut before any message_delta: what it had, and not final.
        let mut r = Reader::default();
        r.feed(&sse.as_bytes()[..200]);
        let (_, u, last) = r.result().unwrap();
        assert_eq!((u.input, u.output, last), (10, 1, false));
        let mut r = Reader::default();
        r.feed(br#"{"type":"message","model":"claude-haiku-4-5","usage":{"input_tokens":3,"output_tokens":"#);
        r.feed(br#"5}}"#);
        let (model, u, last) = r.result().unwrap();
        assert_eq!((model.as_str(), u.input, u.output, last), ("claude-haiku-4-5", 3, 5, true));
        // A JSON body cut short holds no usage at all.
        let mut r = Reader::default();
        r.feed(br#"{"type":"message","model":"claude-haiku-4-5","usage":{"input_tokens":3,"output"#);
        assert!(r.result().is_none());
        let mut r = Reader::default();
        r.feed(br#"{"type":"error","error":{"type":"overloaded_error"}}"#);
        assert!(r.result().is_none());
    }

    #[test]
    fn months() {
        let at = 1_792_800_000_000; // 2026-10-24
        assert_eq!(month_of(at), "2026-10");
        assert_eq!(day_of(month_start(at)), "2026-10-01");
        assert_eq!(day_of(next_month(at)), "2026-11-01");
        let dec = parse_month("2026-12").unwrap();
        assert_eq!(day_of(next_month(dec)), "2027-01-01");
        assert_eq!(month_start(dec + 5 * 86_400_000), dec);
        assert_eq!(parse_month("2026-13"), None);
        assert_eq!(parse_month("26-01"), None);
        assert_eq!(day_of(0), "1970-01-01");
    }
}
