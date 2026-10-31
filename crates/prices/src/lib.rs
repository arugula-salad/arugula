//! What Claude tokens cost at Anthropic's list prices: the one price table
//! the platform's broker settles hosted calls at and the daemon estimates a
//! Claude Code session's cost with (#398).
//!
//! - Prices are USD per million tokens, keyed by model id prefix, most
//!   specific first ([`PRICES`], then [`RETIRED`]). A transcript's
//!   `claude-opus-5-5[1m]` is `claude-opus-5-5`'s row: a 1M window costs
//!   the same per token on every model since Claude 4.6.
//! - Cache writes cost 1.25× input for the five-minute cache and 2× for the
//!   hour; cache reads have their own column.
//! - A row can price prompts over a size at other rates ([`Over`]: Claude
//!   Haiku 5.5's over 100,000 tokens). A prompt is every input token of the
//!   request, cache reads and writes included, and the whole request pays
//!   the higher rates.
//! - Fast mode doubles the price of the models that have it ([`FAST`]).
//! - [`lookup`] says nothing for a model the table doesn't name. What to do
//!   then is the caller's: the broker charges the dearest row ([`dearest`])
//!   so a new model is never given away; an estimate shows tokens and no
//!   dollars, never a wrong figure.
//!
//! Not priced: the 1.1× for US-only inference (`inference_geo`), the batch
//! discount, and the partner clouds' own prices (Bedrock's and Vertex's ids
//! aren't in the table, so they get no figure).

use serde_json::Value;

/// One row of the table: a model id prefix, then USD per million input,
/// output and cache-read tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub prefix: &'static str,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    /// Other rates for a prompt over a size.
    pub over: Option<Over>,
}

/// A row's rates for a prompt of more than `above` tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Over {
    pub above: u64,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

const fn row(prefix: &'static str, input: f64, output: f64, cache_read: f64) -> Price {
    Price { prefix, input, output, cache_read, over: None }
}

/// Anthropic's API list prices, from
/// <https://platform.claude.com/docs/en/about-claude/pricing> as of
/// 2026-10-31. Check it against that page whenever a model is added or its
/// price changes: a stale row miscounts every account on the platform's
/// key, and every estimate.
pub const PRICES: [Price; 13] = [
    row("claude-fable-5-1", 10.0, 50.0, 0.25),
    row("claude-fable-5", 10.0, 50.0, 1.0),
    row("claude-mythos-5-1", 10.0, 50.0, 0.25),
    row("claude-mythos-5", 10.0, 50.0, 1.0),
    row("claude-opus-5-5", 4.0, 20.0, 0.2),
    row("claude-opus-5", 5.0, 25.0, 0.5),
    // Its own row only so FAST can name it.
    row("claude-opus-4-8", 5.0, 25.0, 0.5),
    // Opus 4.5 to 4.7; 4.1 and 4 are in RETIRED, which is read first.
    row("claude-opus-4", 5.0, 25.0, 0.5),
    row("claude-sonnet-5-5", 2.0, 10.0, 0.1),
    row("claude-sonnet-5", 2.0, 10.0, 0.2),
    row("claude-sonnet-4", 3.0, 15.0, 0.3),
    Price {
        prefix: "claude-haiku-5-5",
        input: 0.1,
        output: 0.5,
        cache_read: 0.01,
        over: Some(Over { above: 100_000, input: 0.5, output: 2.5, cache_read: 0.05 }),
    },
    row("claude-haiku-4-5", 1.0, 5.0, 0.1),
];

/// Models Anthropic's API no longer serves, priced so an old transcript's
/// figure is right. Read before [`PRICES`] (they'd fall under its
/// `claude-opus-4`), and never [`dearest`]: a model the broker is asked for
/// now isn't one of these.
pub const RETIRED: [Price; 3] = [
    row("claude-opus-4-1", 15.0, 75.0, 1.5),
    // Claude Opus 4: `claude-opus-4-0`, `claude-opus-4-20250514`.
    row("claude-opus-4-0", 15.0, 75.0, 1.5),
    row("claude-opus-4-2025", 15.0, 75.0, 1.5),
];

/// The models whose fast mode doubles their price.
pub const FAST: [&str; 3] = ["claude-opus-5-5", "claude-opus-5", "claude-opus-4-8"];

/// What one web search the server tool makes costs: $10 per thousand
/// (`usage.server_tool_use.web_search_requests`). Web fetch costs only the
/// tokens it adds.
pub const WEB_SEARCH_USD: f64 = 0.01;

/// The row for a model id, or nothing when the table doesn't name it.
pub fn lookup(model: &str) -> Option<Price> {
    let model = model.trim();
    RETIRED.into_iter().chain(PRICES).find(|p| model.starts_with(p.prefix))
}

/// The row with the dearest output (of the models served now).
pub fn dearest() -> Price {
    PRICES.into_iter().reduce(|a, b| if b.output > a.output { b } else { a }).expect("a price table")
}

impl Price {
    /// Its dearest rates: what a reservation that must never fall short
    /// is made at.
    pub fn dearest_rates(&self) -> (f64, f64) {
        match self.over {
            Some(o) => (self.input.max(o.input), self.output.max(o.output)),
            None => (self.input, self.output),
        }
    }
}

/// What a Messages API response said it used: its `usage`, as the API
/// reports it. Adding them up keeps the counts; `fast` is per response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// From a response's `usage` object: the one-hour writes from
    /// `cache_creation`, and the five-minute ones from there when it says,
    /// else what's left of `cache_creation_input_tokens`.
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

    /// Every input token of the request: what a prompt's size is.
    pub fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_write_1h + self.cache_read
    }

    /// Both cache writes.
    pub fn cache_writes(&self) -> u64 {
        self.cache_write + self.cache_write_1h
    }

    /// Nothing counted.
    pub fn is_empty(&self) -> bool {
        self.prompt() + self.output + self.web_searches == 0
    }

    /// The counts of both.
    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
        self.web_searches += o.web_searches;
    }
}

/// USD for one response that used `usage`, at `row`'s prices.
pub fn cost(row: &Price, usage: &Usage) -> f64 {
    let (input, output, cache_read) = match row.over {
        Some(o) if usage.prompt() > o.above => (o.input, o.output, o.cache_read),
        _ => (row.input, row.output, row.cache_read),
    };
    let times = if usage.fast && FAST.contains(&row.prefix) { 2.0 } else { 1.0 };
    let tokens = usage.input as f64 * input
        + usage.output as f64 * output
        + usage.cache_write as f64 * input * 1.25
        + usage.cache_write_1h as f64 * input * 2.0
        + usage.cache_read as f64 * cache_read;
    tokens * times / 1e6 + usage.web_searches as f64 * WEB_SEARCH_USD
}

/// USD for one response of `model`, or nothing when the table doesn't name
/// the model.
pub fn estimate(model: &str, usage: &Usage) -> Option<f64> {
    lookup(model).map(|row| cost(&row, usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usd(model: &str, u: Value) -> f64 {
        estimate(model, &Usage::from_json(&u)).expect("a priced model")
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A million tokens of each kind at each column's price, as the pricing
    /// page lists them.
    #[test]
    fn each_kind_of_token_at_its_own_price() {
        let m = 1_000_000;
        assert!(close(usd("claude-opus-5-5", json!({ "input_tokens": m })), 4.0));
        assert!(close(usd("claude-opus-5-5", json!({ "output_tokens": m })), 20.0));
        assert!(close(usd("claude-opus-5-5", json!({ "cache_read_input_tokens": m })), 0.2));
        assert!(close(usd("claude-opus-5-5", json!({ "cache_creation_input_tokens": m })), 5.0), "five-minute writes");
        let hour = json!({ "cache_creation_input_tokens": m, "cache_creation": { "ephemeral_1h_input_tokens": m } });
        assert!(close(usd("claude-opus-5-5", hour), 8.0), "hour writes");
        assert!(close(usd("claude-opus-5-20261001", json!({ "input_tokens": m })), 5.0), "most specific first");
        assert!(close(usd("claude-sonnet-5-5", json!({ "cache_read_input_tokens": m })), 0.1));
        assert!(close(usd("claude-sonnet-5", json!({ "cache_read_input_tokens": m })), 0.2));
        assert!(close(usd("claude-fable-5-1", json!({ "cache_read_input_tokens": m })), 0.25));
        assert!(close(usd("claude-fable-5", json!({ "cache_read_input_tokens": m })), 1.0));
        assert!(close(usd("claude-mythos-5-1", json!({ "cache_read_input_tokens": m })), 0.25));
        assert!(close(usd("claude-sonnet-4-6", json!({ "output_tokens": m })), 15.0));
        assert!(close(usd("claude-haiku-4-5-20251001", json!({ "input_tokens": m })), 1.0));
        assert!(close(usd("claude-opus-4-7", json!({ "output_tokens": m })), 25.0));
    }

    #[test]
    fn retired_opus_4_and_4_1_are_their_own_price() {
        assert!(close(usd("claude-opus-4-1-20250805", json!({ "output_tokens": 1_000_000 })), 75.0));
        assert!(close(usd("claude-opus-4-20250514", json!({ "input_tokens": 1_000_000 })), 15.0));
        assert!(close(usd("claude-opus-4-0", json!({ "input_tokens": 1_000_000 })), 15.0));
        assert!(close(usd("claude-opus-4-5-20251101", json!({ "input_tokens": 1_000_000 })), 5.0));
        // They're never what an unknown model is charged as.
        assert_eq!(dearest().prefix, "claude-fable-5-1");
    }

    #[test]
    fn a_1m_window_costs_the_same_per_token() {
        let u = json!({ "input_tokens": 2, "cache_read_input_tokens": 600_000, "output_tokens": 1000 });
        let marked = usd("claude-opus-5-5[1m]", u.clone());
        assert!(close(marked, usd("claude-opus-5-5", u)));
        assert!(close(marked, (2.0 * 4.0 + 600_000.0 * 0.2 + 1000.0 * 20.0) / 1e6));
    }

    #[test]
    fn haiku_5_5_prices_a_prompt_over_100k_at_its_higher_rates() {
        // 100,000 exactly is the lower rate.
        let at = json!({ "input_tokens": 50_000, "cache_read_input_tokens": 50_000, "output_tokens": 1_000_000 });
        assert!(close(usd("claude-haiku-5-5", at), (50_000.0 * 0.1 + 50_000.0 * 0.01 + 1e6 * 0.5) / 1e6));
        // One more, and every token of it is the higher rate, cache reads
        // and writes counted in the prompt.
        let over = json!({ "input_tokens": 1, "cache_read_input_tokens": 50_000, "cache_creation_input_tokens": 50_000,
                           "output_tokens": 1_000_000 });
        let want = (1.0 * 0.5 + 50_000.0 * 0.05 + 50_000.0 * 0.5 * 1.25 + 1e6 * 2.5) / 1e6;
        assert!(close(usd("claude-haiku-5-5", over), want));
        assert_eq!(lookup("claude-haiku-5-5").unwrap().dearest_rates(), (0.5, 2.5));
    }

    #[test]
    fn fast_mode_doubles_the_models_that_have_it() {
        let fast = json!({ "output_tokens": 1_000_000, "speed": "fast" });
        assert!(close(usd("claude-opus-5-5", fast.clone()), 40.0));
        assert!(close(usd("claude-opus-5", fast.clone()), 50.0));
        assert!(close(usd("claude-opus-4-8", fast.clone()), 50.0));
        assert!(close(usd("claude-opus-4-7", fast), 25.0), "no fast mode, no double");
    }

    #[test]
    fn an_unknown_model_has_no_price() {
        assert_eq!(lookup("claude-next-9"), None);
        assert_eq!(lookup("us.anthropic.claude-opus-5-5-v1:0"), None, "a partner cloud's own prices");
        assert_eq!(lookup(""), None);
        assert_eq!(lookup("<synthetic>"), None);
    }

    #[test]
    fn counts_from_json() {
        let both = json!({ "cache_creation": { "ephemeral_5m_input_tokens": 1_000_000, "ephemeral_1h_input_tokens": 0 },
                           "cache_creation_input_tokens": 9_000_000 });
        assert!(close(usd("claude-haiku-4-5", both), 1.25), "the split wins over the total");
        assert!(close(usd("claude-haiku-4-5", json!({ "input_tokens": -5, "output_tokens": "x" })), 0.0));
        let search = json!({ "server_tool_use": { "web_search_requests": 3 } });
        assert!(close(usd("claude-haiku-4-5", search), 0.03), "web searches at $10 a thousand");
        let mut a = Usage::from_json(&json!({ "input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 3 }));
        a.add(&Usage::from_json(&json!({ "input_tokens": 10, "cache_creation_input_tokens": 4 })));
        assert_eq!((a.input, a.output, a.cache_read, a.cache_writes(), a.prompt()), (11, 2, 3, 4, 18));
        assert!(Usage::default().is_empty());
    }
}
