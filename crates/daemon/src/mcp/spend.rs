//! What agent blocks have spent, as the tools say it (#840): the block's
//! own `spend` (its cost where the agent reports one, and its tokens),
//! written as `spent $1.23 · 45.6k tokens`, and the total over the blocks a
//! caller started. Money is never added across currencies.

use std::collections::BTreeMap;

use arugula_proto::{BlockType, PaneId, api::PaneSummary};
use serde_json::{Value, json};

use super::tools::{Call, Out};

/// An agent block's cost, when its agent reports one.
#[derive(Debug, Clone, PartialEq)]
pub struct Cost {
    pub total: f64,
    /// `USD` when the agent didn't say.
    pub currency: String,
}

/// What one agent block has spent.
#[derive(Debug, Clone, PartialEq)]
pub struct Spend {
    pub cost: Option<Cost>,
    pub tokens: u64,
    /// The block's `cost` and `tokens`, as its state has them.
    json: (Value, Value),
}

impl Spend {
    /// From the block's `spend` call (`{cost, tokens}`).
    pub fn from_value(v: &Value) -> Self {
        let cost = v["cost"]["total"].as_f64().map(|total| Cost {
            total,
            currency: v["cost"]["currency"].as_str().filter(|c| !c.is_empty()).unwrap_or("USD").to_owned(),
        });
        Self {
            cost,
            tokens: v["tokens"]["total"].as_u64().unwrap_or(0),
            json: (v["cost"].clone(), v["tokens"].clone()),
        }
    }

    /// Nothing has been reported yet.
    pub fn is_empty(&self) -> bool {
        self.tokens == 0 && self.cost.as_ref().is_none_or(|c| c.total == 0.0)
    }

    /// `spent $1.23 · 45.6k tokens`; `45.6k tokens · cost not reported` when
    /// the agent sends no cost; `None` before anything is spent.
    pub fn line(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        Some(match (&self.cost, self.tokens) {
            (Some(c), 0) => format!("spent {}", money(c)),
            (Some(c), t) => format!("spent {} · {} tokens", money(c), count(t)),
            (None, t) => format!("{} tokens · cost not reported", count(t)),
        })
    }

    /// Adds the `cost` and `tokens` objects to a result's JSON, as the block's
    /// state has them, with the line as `spent`.
    pub fn put(&self, v: &mut Value) {
        let Some(line) = self.line() else { return };
        v["cost"] = self.json.0.clone();
        v["tokens"] = self.json.1.clone();
        v["spent"] = Value::String(line);
    }
}

/// `$1.23`, or `1.23 EUR` for a currency that isn't dollars.
fn money(c: &Cost) -> String {
    if c.currency == "USD" { format!("${:.2}", c.total) } else { format!("{:.2} {}", c.total, c.currency) }
}

/// `950`, `45.6k`, `1.2M`.
fn count(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..1_000_000 => format!("{:.1}k", n as f64 / 1000.0).replace(".0k", "k"),
        _ => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
    }
}

/// `Agent blocks you started: $4.10 · 210k tokens (3 blocks; 1 doesn't report
/// cost)`, over the blocks that have spent something; `None` when none has.
/// Money is grouped by currency.
pub fn total(who: &str, spends: &[Spend]) -> Option<String> {
    let spends: Vec<&Spend> = spends.iter().filter(|s| !s.is_empty()).collect();
    if spends.is_empty() {
        return None;
    }
    let mut by_currency: BTreeMap<&str, f64> = BTreeMap::new();
    for c in spends.iter().filter_map(|s| s.cost.as_ref()) {
        *by_currency.entry(&c.currency).or_default() += c.total;
    }
    let sums: Vec<String> = by_currency
        .into_iter()
        .map(|(currency, total)| money(&Cost { total, currency: currency.to_owned() }))
        .collect();
    let tokens: u64 = spends.iter().map(|s| s.tokens).sum();
    let blocks = spends.len();
    let silent = spends.iter().filter(|s| s.cost.is_none()).count();
    let mut parts = vec![];
    if !sums.is_empty() {
        parts.push(sums.join(", "));
    }
    parts.push(format!("{} tokens", count(tokens)));
    let mut note = format!("{blocks} block{}", if blocks == 1 { "" } else { "s" });
    if silent == blocks {
        note.push_str("; none report cost");
    } else if silent == 1 {
        note.push_str("; 1 doesn't report cost");
    } else if silent > 1 {
        note.push_str(&format!("; {silent} don't report cost"));
    }
    Some(format!("{who}: {} ({note})", parts.join(" · ")))
}

impl Call<'_> {
    /// An agent block's spend, when it is one that can say.
    pub async fn spend_of(&self, pane: PaneId) -> Option<Spend> {
        let block = self.app.mux.api(|r| crate::mux::Api::Block(pane, r)).await.flatten()?;
        let v = block.call("spend", Value::Null).await.ok()?;
        Some(Spend::from_value(&v))
    }

    /// Whether this caller started `p`: for an agent block, the blocks whose
    /// starter is it; for anyone else, those its client started.
    fn started_here(&self, p: &PaneSummary) -> bool {
        let Some(by) = &p.info.started_by else { return false };
        match self.me() {
            Some(me) => by.block == Some(me),
            None => by.block.is_none() && by.by == self.by(),
        }
    }

    /// A result about agent block `pane` with what it has spent: its line at
    /// the end of the `summary`, and its `cost` and `tokens`.
    pub async fn with_spend_of(&self, pane: PaneId, out: Out) -> Out {
        let mut v = out?;
        if let Some(spend) = self.spend_of(pane).await
            && let Some(line) = spend.line()
        {
            spend.put(&mut v);
            v["summary"] = json!(format!("{} · {line}", v["summary"].as_str().unwrap_or_default()));
        }
        Ok(v)
    }

    /// `list`'s answer with each agent block's spend, and the total of the
    /// blocks this caller started as its last line.
    pub async fn with_spend(&self, out: Out) -> Out {
        let mut v = out?;
        let panes = self.panes().await;
        let mut started = vec![];
        let ids: Vec<(usize, PaneId)> = v["panes"]
            .as_array()
            .map(|a| a.iter().enumerate().filter_map(|(i, e)| Some((i, e["pane"].as_u64()? as PaneId))).collect())
            .unwrap_or_default();
        for (i, id) in ids {
            let Some(p) = panes.iter().find(|p| p.info.id == id).filter(|p| p.info.kind == BlockType::Agent) else {
                continue;
            };
            let Some(spend) = self.spend_of(id).await else { continue };
            spend.put(&mut v["panes"][i]);
            if self.started_here(p) {
                started.push(spend);
            }
        }
        if let Some(line) = total("Agent blocks you started", &started) {
            let summary = v["summary"].as_str().unwrap_or_default();
            v["summary"] = json!(format!("{summary}\n{line}"));
            v["agent_spend"] = json!(line);
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spend(cost: Option<(f64, &str)>, tokens: u64) -> Spend {
        Spend::from_value(&json!({
            "cost": cost.map(|(total, currency)| json!({ "total": total, "currency": currency })),
            "tokens": { "total": tokens },
        }))
    }

    #[test]
    fn a_block_says_what_it_spent() {
        assert_eq!(spend(Some((1.234, "USD")), 45_600).line().unwrap(), "spent $1.23 · 45.6k tokens");
        assert_eq!(spend(None, 45_600).line().unwrap(), "45.6k tokens · cost not reported");
        assert_eq!(spend(Some((1.23, "EUR")), 950).line().unwrap(), "spent 1.23 EUR · 950 tokens");
        assert_eq!(spend(None, 0).line(), None);
    }

    #[test]
    fn the_total_keeps_currencies_apart() {
        let all = [spend(Some((1.5, "USD")), 100_000), spend(Some((2.6, "USD")), 100_000), spend(None, 10_000)];
        assert_eq!(
            total("Agent blocks you started", &all).unwrap(),
            "Agent blocks you started: $4.10 · 210k tokens (3 blocks; 1 doesn't report cost)"
        );
        let mixed = [spend(Some((1.0, "USD")), 1000), spend(Some((2.0, "EUR")), 1000)];
        assert_eq!(total("x", &mixed).unwrap(), "x: 2.00 EUR, $1.00 · 2k tokens (2 blocks)");
        assert_eq!(total("x", &[spend(None, 0)]), None);
    }
}
