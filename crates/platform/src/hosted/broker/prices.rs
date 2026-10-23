//! What a Claude call costs at Anthropic's list prices, and how its usage
//! is read from the answer as it streams by: studio's lobby's
//! `lobby/metering.mjs` (studio-018, studio-058), ported as it is, so a
//! cap counts the same dollars on the platform as it did in the lobby.
//!
//! - Prices are USD per million tokens, in one table keyed by model id
//!   prefix, most specific first ([`PRICES`]).
//! - A model the table doesn't name is priced as the dearest row, and
//!   logged once, so a new model is only ever over-counted, never given
//!   away.
//! - Cache writes cost 1.25× input for the five-minute cache and 2× for
//!   the hour; cache reads have their own column.
//! - Fast mode doubles the price of the models that have it ([`FAST`]).

use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};

use serde_json::Value;
use tracing::info;

/// One row of [`PRICES`]: a model id prefix, then USD per million input,
/// output and cache-read tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub prefix: &'static str,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

const fn row(prefix: &'static str, input: f64, output: f64, cache_read: f64) -> Price {
    Price { prefix, input, output, cache_read }
}

/// Anthropic's API prices as of 2026-09-28, the lobby's table row for row
/// (`PRICES` in studio's `lobby/metering.mjs`). Check it against
/// Anthropic's pricing page whenever a model is added or its price
/// changes: a stale row over- or under-charges every account on the
/// platform's key, and the lobby's copy goes when it calls the broker
/// (M236).
pub const PRICES: [Price; 10] = [
    row("claude-fable-5-1", 10.0, 50.0, 0.25),
    row("claude-fable-5", 10.0, 50.0, 0.25),
    row("claude-mythos-5", 10.0, 50.0, 0.25),
    row("claude-opus-5-5", 4.0, 20.0, 0.2),
    row("claude-opus-5", 5.0, 25.0, 0.5),
    row("claude-opus-4", 5.0, 25.0, 0.5),
    row("claude-sonnet-5-5", 2.0, 10.0, 0.2),
    row("claude-sonnet-5", 2.0, 10.0, 0.2),
    row("claude-sonnet-4", 3.0, 15.0, 0.3),
    row("claude-haiku-4-5", 1.0, 5.0, 0.1),
];

/// The models whose fast mode doubles their price.
pub const FAST: [&str; 1] = ["claude-opus-5-5"];

/// What one web search the server tool makes costs: $10 per thousand
/// (`usage.server_tool_use.web_search_requests`). Web fetch costs only the
/// tokens it adds.
pub const WEB_SEARCH_USD: f64 = 0.01;

/// What a model the table doesn't name is priced as: the row with the
/// dearest output, never a cheaper one.
pub fn dearest() -> Price {
    PRICES.into_iter().reduce(|a, b| if b.output > a.output { b } else { a }).expect("a price table")
}

/// The price row for a model id, and whether the table named it. An
/// unknown id is logged the first time it's seen.
pub fn price_of(model: &str) -> (Price, bool) {
    if let Some(p) = PRICES.into_iter().find(|p| model.starts_with(p.prefix)) {
        return (p, true);
    }
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if seen.lock().map(|mut s| s.insert(model.to_owned())).unwrap_or(false) {
        let d = dearest();
        info!(
            model = if model.is_empty() { "a response with no model" } else { model },
            priced_as = d.prefix,
            "a model isn't in the broker's price table; priced as the dearest until it names it"
        );
    }
    (dearest(), false)
}

/// What a Messages API response said it used: its `usage`, as the API
/// reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    /// Five-minute cache writes.
    pub cache_write: u64,
    /// One-hour cache writes.
    pub cache_write_1h: u64,
    pub cache_read: u64,
    /// `usage.server_tool_use.web_search_requests`.
    pub web_searches: u64,
    /// `usage.speed == "fast"`.
    pub fast: bool,
}

/// A count from the API: a positive number, else nothing.
fn n(v: Option<&Value>) -> u64 {
    match v.and_then(Value::as_f64) {
        Some(f) if f.is_finite() && f > 0.0 => f as u64,
        _ => 0,
    }
}

impl Usage {
    /// From a response's `usage` object, as the lobby reads one: the
    /// one-hour writes from `cache_creation`, and the five-minute ones
    /// from there when it says, else what's left of
    /// `cache_creation_input_tokens`.
    pub fn from_json(u: &Value) -> Usage {
        let created = u.get("cache_creation");
        let hour = n(created.and_then(|c| c.get("ephemeral_1h_input_tokens")));
        let five = match created.and_then(|c| c.get("ephemeral_5m_input_tokens")) {
            Some(v) => n(Some(v)),
            None => n(u.get("cache_creation_input_tokens")).saturating_sub(hour),
        };
        Usage {
            input: n(u.get("input_tokens")),
            output: n(u.get("output_tokens")),
            cache_write: five,
            cache_write_1h: hour,
            cache_read: n(u.get("cache_read_input_tokens")),
            web_searches: n(u.get("server_tool_use").and_then(|s| s.get("web_search_requests"))),
            fast: u.get("speed").and_then(Value::as_str) == Some("fast"),
        }
    }
}

/// USD for one response of `model` that used `usage`, and the row it was
/// priced at.
pub fn cost_of(model: &str, usage: &Usage) -> (f64, &'static str) {
    let (row, _) = price_of(model);
    let times = if usage.fast && FAST.contains(&row.prefix) { 2.0 } else { 1.0 };
    let tokens = usage.input as f64 * row.input
        + usage.output as f64 * row.output
        + usage.cache_write as f64 * row.input * 1.25
        + usage.cache_write_1h as f64 * row.input * 2.0
        + usage.cache_read as f64 * row.cache_read;
    (tokens * times / 1e6 + usage.web_searches as f64 * WEB_SEARCH_USD, row.prefix)
}

/// What a Messages request is reserved at before it goes out, in
/// millionths of a dollar: as if every byte of its body below
/// `max_tokens` were a third of a token of input written to the one-hour
/// cache (the dearest input there is), and it used all of `max_tokens`,
/// at `model`'s prices (the dearest row for a model the table doesn't
/// name), doubled in fast mode. With no `max_tokens` (Anthropic refuses
/// those), `NO_MAX` of them.
pub fn reservation(model: &str, body_bytes: usize, max_tokens: Option<u64>, fast: bool) -> u64 {
    const NO_MAX: u64 = 128_000;
    let (row, _) = price_of(model);
    let input = (body_bytes as u64).div_ceil(3) as f64;
    let output = max_tokens.unwrap_or(NO_MAX) as f64;
    let times = if fast && FAST.contains(&row.prefix) { 2.0 } else { 1.0 };
    micros((input * row.input * 2.0 + output * row.output) * times / 1e6)
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

    /// The lobby's sums (`costOf` in studio's `lobby/metering.mjs`): a
    /// million tokens of each kind at each column's price, cache writes at
    /// 1.25× and 2× input, fast mode doubling only the models that have it,
    /// and an unknown model at the dearest row.
    #[test]
    fn prices_are_the_lobbys() {
        assert!(close(usd("claude-opus-5-20261001", json!({ "input_tokens": 1_000_000 })), 5.0));
        assert!(close(usd("claude-opus-5-5", json!({ "input_tokens": 1_000_000 })), 4.0), "most specific first");
        assert!(close(usd("claude-sonnet-4-6", json!({ "output_tokens": 1_000_000 })), 15.0));
        assert!(close(usd("claude-haiku-4-5", json!({ "cache_read_input_tokens": 1_000_000 })), 0.1));
        assert!(close(usd("claude-haiku-4-5", json!({ "cache_creation_input_tokens": 1_000_000 })), 1.25));
        let hour = json!({ "cache_creation_input_tokens": 3_000_000,
                           "cache_creation": { "ephemeral_1h_input_tokens": 1_000_000 } });
        assert!(close(usd("claude-haiku-4-5", hour), 2.0 * 1.25 + 2.0), "what's left is the five-minute cache");
        let both = json!({ "cache_creation": { "ephemeral_5m_input_tokens": 1_000_000, "ephemeral_1h_input_tokens": 0 },
                           "cache_creation_input_tokens": 9_000_000 });
        assert!(close(usd("claude-haiku-4-5", both), 1.25), "the split wins over the total");
        let fast = json!({ "output_tokens": 1_000_000, "speed": "fast" });
        assert!(close(usd("claude-opus-5-5", fast.clone()), 40.0));
        assert!(close(usd("claude-opus-5", fast), 25.0), "no fast mode, no double");
        let (p, known) = price_of("claude-next-9");
        assert!(!known);
        assert_eq!(p.prefix, "claude-fable-5-1");
        assert!(close(usd("claude-next-9", json!({ "output_tokens": 1_000_000 })), 50.0));
        assert!(close(usd("claude-haiku-4-5", json!({ "input_tokens": -5, "output_tokens": "x" })), 0.0));
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
