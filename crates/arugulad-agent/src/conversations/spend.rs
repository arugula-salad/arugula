//! What a Claude Code session cost, estimated (#398): its transcript's
//! token counts at Anthropic's API list prices (`arugula-prices`, the
//! table the platform's broker settles hosted calls at). It's an estimate
//! and never a bill: someone on a Claude subscription doesn't pay per
//! token, and everyone is shown the same figure, "at API prices".
//!
//! - **Responses, not lines.** Claude Code writes one line per content
//!   block of a response, each with the response's `usage` so far, and
//!   the same `message.id`; the lines of one response come together, and
//!   the last says the most. So each response is counted once, at its last
//!   line ([`Spend`]).
//! - **Each response at its own model's prices**, its prompt's size (its
//!   own token counts, never a window guessed from the model's name)
//!   choosing a row's long-prompt rates, its `speed` fast mode. A model the
//!   table doesn't name gets no dollars: its tokens are still counted, and
//!   the total says which models it couldn't price instead of a figure
//!   that leaves them out.
//! - **Subagents** write their own transcripts beside the session's
//!   (`<session>/subagents/agent-<id>.jsonl`, with `agent-<id>.meta.json`
//!   naming their type and task), followed by [`Subagents`]. Older Claude
//!   Code wrote them into the session's own file as `isSidechain` lines;
//!   those are counted as subagents too, by their `agentId`.
//! - **The conversations list** prices transcripts off the listing's path
//!   ([`Listed`]): a conversation is listed with its estimate once one has
//!   been worked out for the transcript as it is.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::SystemTime,
};

use arugula_prices::Usage;
use serde_json::{Value, json};

use super::convert::Follow;

/// Where the figures come from, for the client to say.
pub const BASIS: &str = "an estimate from the transcript's token counts at Anthropic's API list prices, not a bill";

/// One agent's responses: their tokens, and what they cost where every
/// model was priced.
#[derive(Clone, Default, Debug)]
pub struct Ledger {
    /// The response being written: its id, model and usage so far.
    open: Option<(Option<String>, String, Usage)>,
    tokens: Usage,
    usd: f64,
    /// Models with tokens and no price.
    unpriced: Vec<String>,
}

impl Ledger {
    fn response(&mut self, id: Option<&str>, model: &str, usage: Usage) {
        match &mut self.open {
            Some((Some(open), _, u)) if Some(open.as_str()) == id => *u = usage,
            _ => {
                self.close();
                self.open = Some((id.map(str::to_owned), model.to_owned(), usage));
            }
        }
    }

    fn close(&mut self) {
        if let Some((_, model, u)) = self.open.take() {
            self.count(&model, &u);
        }
    }

    fn count(&mut self, model: &str, u: &Usage) {
        if u.is_empty() {
            return;
        }
        self.tokens.add(u);
        match arugula_prices::estimate(model, u) {
            Some(usd) => self.usd += usd,
            None if !self.unpriced.iter().any(|m| m == model) => self.unpriced.push(model.to_owned()),
            None => {}
        }
    }

    /// With the open response counted.
    fn settled(&self) -> Ledger {
        let mut l = self.clone();
        l.close();
        l
    }

    fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.open.as_ref().is_none_or(|(_, _, u)| u.is_empty())
    }

    fn merge(&mut self, o: &Ledger) {
        self.tokens.add(&o.tokens);
        self.usd += o.usd;
        for m in &o.unpriced {
            if !self.unpriced.contains(m) {
                self.unpriced.push(m.clone());
            }
        }
    }

    /// `{usd, tokens, unpriced}`; `usd` is null when a model had no price.
    fn json(&self) -> Value {
        let t = &self.tokens;
        json!({
            "usd": if self.unpriced.is_empty() { json!(round(self.usd)) } else { Value::Null },
            "tokens": {
                "input": t.input,
                "output": t.output,
                "cache_write": t.cache_writes(),
                "cache_read": t.cache_read,
                "total": t.prompt() + t.output,
            },
            "unpriced": self.unpriced,
        })
    }
}

/// Dollars to the hundredth of a cent: what an estimate can claim.
fn round(usd: f64) -> f64 {
    (usd * 10_000.0).round() / 10_000.0
}

/// A transcript's responses: the session's own, and any subagents' written
/// into it (`isSidechain`), by their `agentId`.
#[derive(Clone, Default, Debug)]
pub struct Spend {
    main: Ledger,
    inline: BTreeMap<String, Ledger>,
}

impl Spend {
    /// One transcript line, in file order.
    pub fn line(&mut self, v: &Value) {
        if v["type"] != "assistant" {
            return;
        }
        let m = &v["message"];
        let model = m["model"].as_str().unwrap_or("");
        // A synthetic entry (an API error, an interruption) used nothing.
        if model.starts_with('<') || !m["usage"].is_object() {
            return;
        }
        let ledger = if v["isSidechain"] == true {
            let who = v["agentId"].as_str().unwrap_or("subagent").to_owned();
            self.inline.entry(who).or_default()
        } else {
            &mut self.main
        };
        ledger.response(m["id"].as_str(), model, Usage::from_json(&m["usage"]));
    }

    /// The session's own responses, settled.
    pub fn main(&self) -> Ledger {
        self.main.settled()
    }
}

/// A session's subagents' transcripts, each followed as it grows.
#[derive(Default)]
pub struct Subagents {
    files: BTreeMap<PathBuf, Sub>,
}

struct Sub {
    stamp: Option<(u64, SystemTime)>,
    follow: Follow,
    meta: Option<Value>,
}

/// Where a session's subagents' transcripts are.
pub fn subagents_dir(transcript: &Path) -> PathBuf {
    transcript.with_extension("").join("subagents")
}

impl Subagents {
    /// Read what's new in the subagents of the session at `transcript`.
    /// True if anything changed.
    pub fn refresh(&mut self, transcript: &Path) -> bool {
        let dir = subagents_dir(transcript);
        let mut changed = false;
        let mut seen = HashSet::new();
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            seen.insert(path.clone());
            let stamp = e.metadata().ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
            let sub = self.files.entry(path.clone()).or_insert_with(|| Sub {
                stamp: None,
                follow: Follow::context_only(),
                meta: None,
            });
            if sub.meta.is_none() {
                sub.meta = std::fs::read(path.with_extension("meta.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                changed |= sub.meta.is_some();
            }
            if stamp.is_some() && stamp != sub.stamp {
                sub.stamp = stamp;
                changed |= sub.follow.read(&path).unwrap_or(false);
            }
        }
        let before = self.files.len();
        self.files.retain(|p, _| seen.contains(p));
        changed || self.files.len() != before
    }
}

/// The estimate for a session: its total, its own share and each
/// subagent's; or nothing when it has no counted tokens.
pub fn estimate(main: &Spend, subs: &Subagents) -> Option<Value> {
    let own = main.main();
    let mut total = own.clone();
    let mut agents = vec![];
    for (who, l) in &main.inline {
        let l = l.settled();
        if l.is_empty() {
            continue;
        }
        total.merge(&l);
        agents.push(json!({ "id": who, "kind": Value::Null, "description": Value::Null, "cost": l.json() }));
    }
    for (path, sub) in &subs.files {
        let spend = sub.follow.spend();
        let mut l = spend.main();
        // A subagent's own subagents, if it wrote any into its file.
        for i in spend.inline.values() {
            l.merge(&i.settled());
        }
        if l.is_empty() {
            continue;
        }
        total.merge(&l);
        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").trim_start_matches("agent-").to_owned();
        let meta = sub.meta.as_ref();
        agents.push(json!({
            "id": id,
            "kind": meta.and_then(|m| m["agentType"].as_str()),
            "description": meta.and_then(|m| m["description"].as_str()),
            "cost": l.json(),
        }));
    }
    if total.is_empty() {
        return None;
    }
    let mut out = total.json();
    out["session"] = own.json();
    out["subagents"] = json!(agents);
    out["basis"] = json!(BASIS);
    // Where the figure comes from: priced here from the transcripts' tokens.
    // Claude Code's own figure, where its engine reports one, could stand
    // in for it later as another source.
    out["source"] = json!("tokens");
    Some(out)
}

/// The conversations list's estimates (#398), worked out by a thread of
/// their own so a listing never waits on reading whole transcripts. A
/// listing asks for each conversation it shows ([`Listed::get`]), newest
/// first: what's known for the transcript as it is now comes back, and
/// anything else is queued, for the next listing.
pub struct Listed {
    state: Mutex<ListedState>,
    wake: Condvar,
}

#[derive(Default)]
struct ListedState {
    /// By transcript: its size and mtime when priced, and the estimate.
    known: HashMap<PathBuf, ((u64, u64), Option<Value>)>,
    /// To price, in the order asked.
    queue: Vec<(PathBuf, (u64, u64))>,
    started: bool,
}

/// How many transcripts the pricing thread keeps following, the newest:
/// the rest are read again from the start if they change.
const KEEP: usize = 32;

impl Listed {
    pub fn global() -> &'static Arc<Listed> {
        static G: OnceLock<Arc<Listed>> = OnceLock::new();
        G.get_or_init(|| Arc::new(Listed { state: Mutex::default(), wake: Condvar::new() }))
    }

    /// The estimate for the transcript at `path` with this size and mtime
    /// (ms), if one's been worked out; else it's queued.
    pub fn get(self: &Arc<Self>, path: &Path, stamp: (u64, u64)) -> Option<Value> {
        let mut s = self.state.lock().unwrap();
        if let Some((at, v)) = s.known.get(path)
            && *at == stamp
        {
            return v.clone();
        }
        // An older figure stands in until the new one's known.
        let stale = s.known.get(path).and_then(|(_, v)| v.clone());
        if !s.queue.iter().any(|(p, st)| p == path && *st == stamp) {
            s.queue.retain(|(p, _)| p != path);
            s.queue.push((path.to_owned(), stamp));
        }
        if !s.started {
            s.started = true;
            let me = self.clone();
            let spawned =
                std::thread::Builder::new().name("conversation-costs".into()).spawn(move || me.work()).is_ok();
            s.started = spawned;
        }
        self.wake.notify_one();
        stale
    }

    fn work(&self) {
        let mut follows: HashMap<PathBuf, (u64, Follow, Subagents)> = HashMap::new();
        loop {
            let (path, stamp) = {
                let mut s = self.state.lock().unwrap();
                while s.queue.is_empty() {
                    s = self.wake.wait(s).unwrap();
                }
                s.queue.remove(0)
            };
            let (_, follow, subs) =
                follows.entry(path.clone()).or_insert_with(|| (0, Follow::context_only(), Subagents::default()));
            let _ = follow.read(&path);
            subs.refresh(&path);
            let v = estimate(&follow.spend(), subs);
            follows.get_mut(&path).expect("just put").0 = stamp.1;
            if follows.len() > KEEP {
                let mut by_age: Vec<(u64, PathBuf)> = follows.iter().map(|(p, (m, ..))| (*m, p.clone())).collect();
                by_age.sort();
                for (_, p) in by_age.into_iter().take(follows.len() - KEEP) {
                    follows.remove(&p);
                }
            }
            self.state.lock().unwrap().known.insert(path, (stamp, v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    /// One response, written as Claude Code does: a line per content block,
    /// the usage growing to its last.
    fn response(id: &str, model: &str, usage: Value, blocks: usize) -> Vec<Value> {
        (0..blocks)
            .map(|i| {
                let mut u = usage.clone();
                if i + 1 < blocks {
                    u["output_tokens"] = json!(1);
                }
                json!({"type": "assistant", "uuid": format!("{id}-{i}"), "timestamp": "2026-10-31T10:00:00.000Z",
                    "message": {"id": id, "model": model, "role": "assistant", "usage": u,
                        "content": [{"type": "text", "text": "…"}]}})
            })
            .collect()
    }

    fn spend(lines: &[Value]) -> Spend {
        let mut s = Spend::default();
        for l in lines {
            s.line(l);
        }
        s
    }

    fn est(lines: &[Value]) -> Value {
        estimate(&spend(lines), &Subagents::default()).expect("an estimate")
    }

    #[test]
    fn a_plain_session_counts_each_response_once_at_its_last_line() {
        let mut lines = response("m1", "claude-opus-5-5", json!({"input_tokens": 1000, "output_tokens": 500}), 3);
        lines.extend(response("m2", "claude-opus-5-5", json!({"input_tokens": 2000, "output_tokens": 1000}), 2));
        lines.push(json!({"type": "user", "message": {"content": "next"}}));
        let e = est(&lines);
        assert_eq!(e["tokens"]["input"], 3000);
        assert_eq!(e["tokens"]["output"], 1500);
        assert!(close(e["usd"].as_f64().unwrap(), (3000.0 * 4.0 + 1500.0 * 20.0) / 1e6));
        assert_eq!(e["session"], json!({"usd": e["usd"], "tokens": e["tokens"], "unpriced": []}));
        assert_eq!(e["subagents"], json!([]));
        assert_eq!(e["basis"], BASIS);
        assert_eq!(e["source"], "tokens");
    }

    #[test]
    fn a_cache_heavy_session_prices_reads_and_writes_on_their_own() {
        let u = json!({"input_tokens": 2, "output_tokens": 300, "cache_read_input_tokens": 90_000,
            "cache_creation_input_tokens": 30_000,
            "cache_creation": {"ephemeral_1h_input_tokens": 20_000, "ephemeral_5m_input_tokens": 10_000}});
        let e = est(&response("m1", "claude-sonnet-5-5", u, 2));
        assert_eq!(e["tokens"]["cache_read"], 90_000);
        assert_eq!(e["tokens"]["cache_write"], 30_000);
        let want = (2.0 * 2.0 + 300.0 * 10.0 + 90_000.0 * 0.1 + 10_000.0 * 2.0 * 1.25 + 20_000.0 * 2.0 * 2.0) / 1e6;
        assert!(close(e["usd"].as_f64().unwrap(), want), "{e}");
    }

    #[test]
    fn a_1m_session_is_priced_as_its_model() {
        let u = json!({"input_tokens": 5, "output_tokens": 2000, "cache_read_input_tokens": 700_000});
        let e = est(&response("m1", "claude-opus-5-5[1m]", u, 1));
        assert!(close(e["usd"].as_f64().unwrap(), (5.0 * 4.0 + 2000.0 * 20.0 + 700_000.0 * 0.2) / 1e6));
        // Haiku 5.5's prompts over 100k are its dearer rates.
        let u = json!({"input_tokens": 1, "output_tokens": 1000, "cache_read_input_tokens": 150_000});
        let e = est(&response("m1", "claude-haiku-5-5", u, 1));
        assert!(close(e["usd"].as_f64().unwrap(), (1.0 * 0.5 + 1000.0 * 2.5 + 150_000.0 * 0.05) / 1e6));
    }

    #[test]
    fn an_unknown_model_shows_tokens_and_no_dollars() {
        let mut lines = response("m1", "claude-opus-5-5", json!({"input_tokens": 10, "output_tokens": 10}), 1);
        lines.extend(response("m2", "claude-next-9", json!({"input_tokens": 100, "output_tokens": 50}), 1));
        lines.extend(response("m3", "claude-next-9", json!({"input_tokens": 1, "output_tokens": 1}), 1));
        let e = est(&lines);
        assert!(e["usd"].is_null(), "no figure that leaves tokens out: {e}");
        assert_eq!(e["tokens"]["total"], 172);
        assert_eq!(e["unpriced"], json!(["claude-next-9"]));
    }

    #[test]
    fn synthetic_entries_and_empty_sessions_count_nothing() {
        let api_error = json!({"type": "assistant", "isApiErrorMessage": true,
            "message": {"id": "e1", "model": "<synthetic>", "usage": {"input_tokens": 0, "output_tokens": 0}}});
        assert!(estimate(&spend(&[api_error]), &Subagents::default()).is_none());
        assert!(estimate(&Spend::default(), &Subagents::default()).is_none());
    }

    #[test]
    fn inline_subagents_are_their_own_and_in_the_total() {
        let mut lines = response("m1", "claude-opus-5-5", json!({"input_tokens": 1000, "output_tokens": 0}), 1);
        let mut side = response("s1", "claude-haiku-4-5", json!({"input_tokens": 1000, "output_tokens": 0}), 2);
        for l in &mut side {
            l["isSidechain"] = json!(true);
            l["agentId"] = json!("a1");
        }
        lines.extend(side);
        let e = est(&lines);
        assert!(close(e["session"]["usd"].as_f64().unwrap(), 0.004));
        assert_eq!(e["subagents"][0]["id"], "a1");
        assert!(close(e["subagents"][0]["cost"]["usd"].as_f64().unwrap(), 0.001));
        assert!(close(e["usd"].as_f64().unwrap(), 0.005));
    }

    #[test]
    fn subagents_beside_the_transcript_are_followed() {
        let root = std::env::temp_dir().join(format!("arugula-spend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let session = root.join("s1.jsonl");
        let dir = subagents_dir(&session);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |p: &Path, lines: &[Value]| {
            let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
            std::fs::write(p, text).unwrap();
        };
        let main = response("m1", "claude-opus-5-5", json!({"input_tokens": 1_000_000, "output_tokens": 0}), 1);
        write(&session, &main);
        let sub = response("x1", "claude-sonnet-5-5", json!({"input_tokens": 0, "output_tokens": 100_000}), 3);
        write(&dir.join("agent-abc.jsonl"), &sub);
        std::fs::write(dir.join("agent-abc.meta.json"), r#"{"agentType":"Explore","description":"Find the table"}"#)
            .unwrap();
        // One on a model with no price: tokens, no dollars, and no total.
        let odd = response("y1", "claude-next-9", json!({"input_tokens": 7, "output_tokens": 3}), 1);
        write(&dir.join("agent-def.jsonl"), &odd);

        let mut subs = Subagents::default();
        assert!(subs.refresh(&session));
        assert!(!subs.refresh(&session), "nothing new");
        let e = estimate(&spend(&main), &subs).unwrap();
        let a = &e["subagents"][0];
        assert_eq!((a["id"].as_str(), a["kind"].as_str()), (Some("abc"), Some("Explore")));
        assert_eq!(a["description"], "Find the table");
        assert!(close(a["cost"]["usd"].as_f64().unwrap(), 1.0));
        assert!(close(e["session"]["usd"].as_f64().unwrap(), 4.0));
        assert_eq!(e["subagents"][1]["cost"]["tokens"]["total"], 10);
        assert!(e["subagents"][1]["cost"]["usd"].is_null());
        assert!(e["usd"].is_null());
        assert_eq!(e["unpriced"], json!(["claude-next-9"]));

        // It grows; one goes.
        std::fs::remove_file(dir.join("agent-def.jsonl")).unwrap();
        let more = response("x2", "claude-sonnet-5-5", json!({"input_tokens": 0, "output_tokens": 100_000}), 1);
        let mut all = sub.clone();
        all.extend(more);
        write(&dir.join("agent-abc.jsonl"), &all);
        assert!(subs.refresh(&session));
        let e = estimate(&spend(&main), &subs).unwrap();
        assert_eq!(e["subagents"].as_array().unwrap().len(), 1);
        assert!(close(e["usd"].as_f64().unwrap(), 6.0));

        // The list's thread works out the same, off the listing's path.
        let listed = Arc::new(Listed { state: Mutex::default(), wake: Condvar::new() });
        assert_eq!(listed.get(&session, (1, 1)), None, "queued");
        let mut got = None;
        for _ in 0..200 {
            got = listed.get(&session, (1, 1));
            if got.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(got, Some(e));
        let _ = std::fs::remove_dir_all(&root);
    }
}
