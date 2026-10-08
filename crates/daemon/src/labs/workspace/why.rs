//! #619: which decision and run made each hunk in *Changes* on a chant
//! workspace member, from chant's own answer.
//!
//! When a file opens, the diff block asks the workspace's chant `graph
//! --intent <file> --json` (at `rev_b` for a range) and reads its `why`:
//! `git blame` spans over the file's current lines, each with the commit
//! that last wrote it and the agent runs behind that commit, and the
//! decisions ranked by relevance. Nothing here joins what chant can: a
//! hunk takes the spans its added lines fall in, and from them the commits,
//! the runs and the decisions those carry out; when they carry none, the
//! decision chant ranks first for the file. Lines not committed yet say so,
//! and name who holds a lease on the file's work: `status`'s `leases`, by
//! the work items the same walk says cover the file (joined by record id,
//! as the read contract joins documents). A lease on no work item the walk
//! names isn't said.
//!
//! Arugula's own piece is the pane: a run is linked to the open agent
//! block whose recent turns wrote that run id (#590), and a lease's holder
//! to the open agent block of this workspace running as that session.
//! DECISIONS.md lists these joins.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;
use serde_json::Value;

use crate::{block::BlockCtx, review::Runner};

/// `sh -c READ sh ROOT CHANT ARGS…`: one chant read in the workspace root,
/// its document on stdout.
pub const READ: &str = r#"cd "$1" 2>/dev/null || { printf '{"error":{"message":"no such directory: %s"}}' "$1"; exit 0; }
c=$2; shift 2
exec "$c" workspace "$@""#;

/// What one hunk's added lines came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HunkWhy {
    /// Added lines nobody has committed yet.
    pub uncommitted: u32,
    /// Added lines a commit wrote.
    pub committed: u32,
    pub commits: Vec<Commit>,
    pub runs: Vec<Run>,
    pub decisions: Vec<Decision>,
    /// Who holds a lease on the file's work, for uncommitted lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<Holder>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    pub id: String,
    /// The agent session it ran as.
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    /// The agent block that made it, while it's open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    /// The record's id (`why-001`).
    pub id: String,
    pub title: Option<String>,
    pub state: Option<String>,
    /// chant's: `carried` when the hunk's commit or run carries it out,
    /// else how it constrains the file (`path`, `member`, ...).
    pub relevance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Holder {
    pub name: String,
    /// The work item the lease is on.
    pub item: String,
    /// The agent block running as that session, while it's open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
}

/// The open agent blocks of this workspace, for linking runs and leases to
/// panes.
#[derive(Debug, Clone, Default)]
pub struct Panes {
    agents: Vec<Agent>,
}

/// An open agent block started from one of the workspace's members (#304).
#[derive(Debug, Clone)]
struct Agent {
    block: u32,
    /// The agent session it runs as.
    session: Option<String>,
    /// The runs its recent turns wrote to the ledger.
    runs: Vec<String>,
}

impl Panes {
    /// From the open agent blocks' configs and states, those whose
    /// `chant.root` is `root`.
    pub fn of(root: &str, blocks: &[(u32, Value, Value)]) -> Self {
        let same = |r: &str| r.trim_end_matches('/') == root.trim_end_matches('/');
        let agents = blocks
            .iter()
            .filter(|(_, c, _)| c["chant"]["root"].as_str().is_some_and(same))
            .map(|(id, c, st)| Agent {
                block: *id,
                session: c["chant"]["agent"].as_str().map(str::to_owned),
                runs: strings_at(&st["recent_turns"], "run"),
            })
            .collect();
        Self { agents }
    }

    /// The block that wrote `run`, while it's open.
    fn run(&self, run: &str) -> Option<u32> {
        self.agents.iter().find(|a| a.runs.iter().any(|r| r == run)).map(|a| a.block)
    }

    /// The block running as the session `name`.
    fn session(&self, name: &str) -> Option<u32> {
        self.agents.iter().find(|a| a.session.as_deref() == Some(name)).map(|a| a.block)
    }
}

/// One span of blamed lines.
#[derive(Debug, Clone)]
struct Span {
    start: u32,
    end: u32,
    commit: Option<String>,
    runs: Vec<String>,
}

/// A file's `why`, as read: what a hunk is matched against.
#[derive(Debug, Clone, Default)]
pub struct Why {
    spans: Vec<Span>,
    nodes: HashMap<String, Value>,
    /// Decisions in chant's order: (node id, relevance, current).
    ranked: Vec<(String, String, bool)>,
    /// What each commit or run node carries out, by node id.
    carries: HashMap<String, Vec<String>>,
    holder: Option<Holder>,
    panes: Panes,
}

impl Why {
    /// From `graph --intent <file> --json`, and `status --json` for its
    /// leases when lines aren't committed. `Err` with what to say when
    /// chant didn't answer one.
    pub fn read(intent: &Value, status: Option<&Value>, panes: &Panes) -> Result<Self, String> {
        if let Some(e) = intent["error"]["message"].as_str() {
            let code = intent["error"]["code"].as_str().map(|c| format!(" ({c})")).unwrap_or_default();
            return Err(format!("{e}{code}"));
        }
        if intent["contract"] != 1 {
            return Err("chant didn't answer with contract 1".into());
        }
        let why = &intent["why"];
        if !why.is_object() {
            return Err("this chant says nothing about why: chant 0.102.0 or newer does".into());
        }
        let nodes: HashMap<String, Value> = intent["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| Some((n["id"].as_str()?.to_owned(), n.clone())))
            .collect();
        let spans = why["blame"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(Span {
                    start: s["start"].as_u64()? as u32,
                    end: s["end"].as_u64()? as u32,
                    commit: s["commit"].as_str().map(str::to_owned),
                    runs: strings(&s["runs"]),
                })
            })
            .collect();
        let ranked = why["decisions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some((
                    d["decision"].as_str()?.to_owned(),
                    d["relevance"].as_str().unwrap_or("related").to_owned(),
                    d["current"] != false,
                ))
            })
            .collect();
        let mut carries: HashMap<String, Vec<String>> = HashMap::new();
        for e in intent["edges"].as_array().into_iter().flatten() {
            let carried = e["kind"] == "carries" || (e["kind"] == "within" && e["state"] == "decided");
            if let (true, Some(from), Some(to)) = (carried, e["from"].as_str(), e["to"].as_str()) {
                carries.entry(from.to_owned()).or_default().push(to.to_owned());
            }
        }
        for r in why["runs"].as_array().into_iter().flatten() {
            if let Some(id) = r["run"].as_str() {
                carries.entry(id.to_owned()).or_default().extend(strings(&r["decisions"]));
            }
        }
        let work: BTreeSet<String> = nodes
            .values()
            .filter(|n| n["kind"] == "work")
            .filter_map(|n| n["record"].as_str().map(str::to_owned))
            .collect();
        // A live lease on a work item covering the file.
        let holder = status.and_then(|s| {
            let lease = s["leases"]
                .as_array()?
                .iter()
                .find(|l| l["state"] == "active" && l["item"].as_str().is_some_and(|i| work.contains(i)))?;
            let name = lease["holder"].as_str()?.to_owned();
            Some(Holder {
                pane: panes.session(&name),
                item: lease["item"].as_str().unwrap_or_default().to_owned(),
                name,
            })
        });
        Ok(Self { spans, nodes, ranked, carries, holder, panes: panes.clone() })
    }

    /// [`Why::hunk`] as the diff block's state carries it.
    pub fn hunk_json(&self, added: &[u32]) -> Option<Value> {
        self.hunk(added).map(|h| serde_json::to_value(h).unwrap_or_default())
    }

    /// Whether any line isn't committed: then `status` is read for leases.
    pub fn uncommitted(intent: &Value) -> bool {
        intent["why"]["gaps"].as_array().into_iter().flatten().any(|g| g["code"] == "intent-why-uncommitted")
    }

    /// A hunk's why, from the new-side numbers of its added lines (`None`
    /// for a hunk that only removes lines).
    pub fn hunk(&self, added: &[u32]) -> Option<HunkWhy> {
        if added.is_empty() {
            return None;
        }
        let mut out = HunkWhy::default();
        let mut commits: Vec<&str> = Vec::new();
        let mut runs: Vec<&str> = Vec::new();
        for &n in added {
            match self.spans.iter().find(|s| s.start <= n && n <= s.end) {
                Some(Span { commit: Some(c), runs: r, .. }) => {
                    out.committed += 1;
                    if !commits.contains(&c.as_str()) {
                        commits.push(c);
                    }
                    for r in r {
                        if !runs.contains(&r.as_str()) {
                            runs.push(r);
                        }
                    }
                }
                // Blame said nothing about it (the file moved on since the
                // read), or it isn't committed.
                Some(_) => out.uncommitted += 1,
                None => {}
            }
        }
        out.commits = commits
            .iter()
            .map(|id| {
                let n = &self.nodes.get(*id).cloned().unwrap_or_default();
                Commit {
                    sha: n["sha"].as_str().unwrap_or(id.trim_start_matches("commit:")).chars().take(8).collect(),
                    subject: n["subject"].as_str().map(str::to_owned),
                }
            })
            .collect();
        out.runs = runs
            .iter()
            .map(|id| {
                let n = self.nodes.get(*id).cloned().unwrap_or_default();
                let run = n["run"].as_str().unwrap_or(id.trim_start_matches("run:")).to_owned();
                Run {
                    pane: self.panes.run(&run),
                    agent: n["agent"].as_str().map(str::to_owned),
                    harness: n["harness"]["name"].as_str().or(n["harness"].as_str()).map(str::to_owned),
                    model: n["model"].as_str().map(str::to_owned),
                    id: run,
                }
            })
            .collect();
        // What the hunk's commits and runs carry out, in chant's order;
        // else the decision chant ranks first for the file.
        let carried: BTreeSet<&str> = commits
            .iter()
            .chain(runs.iter())
            .flat_map(|id| self.carries.get(*id).into_iter().flatten())
            .map(String::as_str)
            .collect();
        let mut picked: Vec<(&str, &str)> = self
            .ranked
            .iter()
            .filter(|(id, _, _)| carried.contains(id.as_str()))
            .map(|(id, _, _)| (id.as_str(), "carried"))
            .collect();
        let file = |(_, relevance, current): &&(String, String, bool)| *current && relevance != "carried";
        if picked.is_empty()
            && let Some((id, relevance, _)) =
                self.ranked.iter().find(file).or_else(|| self.ranked.iter().find(|(_, _, current)| *current))
        {
            picked.push((id, relevance));
        }
        out.decisions = picked
            .into_iter()
            .map(|(id, relevance)| {
                let n = self.nodes.get(id).cloned().unwrap_or_default();
                Decision {
                    id: n["record"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_owned()),
                    title: n["title"].as_str().map(str::to_owned),
                    state: n["state"].as_str().map(str::to_owned),
                    relevance: relevance.to_owned(),
                }
            })
            .collect();
        if out.uncommitted > 0 {
            out.holder = self.holder.clone();
        }
        Some(out)
    }
}

/// Each object's `key`, where it's a string.
fn strings_at(v: &Value, key: &str) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|o| o[key].as_str().map(str::to_owned)).collect()
}

/// One file's why, asked of the workspace's chant in `root` (`chant`, or
/// `chant` on PATH): `graph --intent <file> --json` (`--at` a revision for
/// a range), and `status local --json` for its leases when lines aren't
/// committed yet; runs and leases linked to this daemon's open agent
/// blocks.
pub async fn read(
    ctx: &BlockCtx,
    root: &str,
    chant: Option<&str>,
    file: &str,
    at: Option<&str>,
) -> Result<Why, String> {
    let runner = Runner::user(ctx).await?;
    let chant = chant.unwrap_or("chant");
    let ask = |args: &[&str]| {
        let mut a = vec![root.to_owned(), chant.to_owned()];
        a.extend(args.iter().map(|s| s.to_string()));
        let runner = runner.clone();
        async move {
            let (out, code) = runner.sh(READ, &a).await?;
            serde_json::from_slice::<Value>(&out).map_err(|_| match code {
                Some(127) => "chant isn't there to run".to_owned(),
                Some(c) => format!("chant exited {c} without saying why"),
                None => "chant was stopped".to_owned(),
            })
        }
    };
    let mut args = vec!["graph", "--intent", file, "--json"];
    if let Some(at) = at {
        args.extend(["--at", at]);
    }
    let intent = ask(&args).await?;
    let status = match Why::uncommitted(&intent) {
        true => ask(&["status", "local", "--json"]).await.ok(),
        false => None,
    };
    let panes = Panes::of(root, &ctx.agents().await);
    Why::read(&intent, status.as_ref(), &panes)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `make.sh` beside them made these with chant 0.108.1: app/server.mjs,
    /// line 3 committed by run `arugula-7-1790848800000` (the member's
    /// agent session `app`), line 7 not committed, why-001 constraining
    /// the file by path, and W-001 on the member, its lease held by `app`.
    const INTENT: &str = include_str!("../../../tests/fixtures/chant/why/intent.json");
    const STATUS: &str = include_str!("../../../tests/fixtures/chant/why/status.json");

    fn docs() -> (Value, Value) {
        (serde_json::from_str(INTENT).unwrap(), serde_json::from_str(STATUS).unwrap())
    }

    /// Agent block 7 runs as `app` in this workspace and wrote the run; 9
    /// is another workspace's, and claims the run too.
    fn panes() -> Panes {
        let turns = json!({ "recent_turns": [{ "run": "arugula-7-1790848800000" }, { "run": "arugula-7-1" }] });
        Panes::of(
            "/ws/",
            &[
                (9, json!({ "chant": { "root": "/elsewhere", "member": "app", "agent": "app" } }), turns.clone()),
                (7, json!({ "chant": { "root": "/ws", "member": "app", "agent": "app" } }), turns),
            ],
        )
    }

    #[test]
    fn a_committed_hunk_names_its_commit_run_and_decision() {
        let (intent, status) = docs();
        let why = Why::read(&intent, Some(&status), &panes()).unwrap();
        let h = why.hunk(&[3]).unwrap();
        assert_eq!((h.committed, h.uncommitted), (1, 0));
        assert_eq!(h.commits.len(), 1);
        assert_eq!(h.commits[0].subject.as_deref(), Some("port: read PORT, default 8080"));
        assert_eq!(h.commits[0].sha.len(), 8);
        assert_eq!(
            h.runs,
            [Run {
                id: "arugula-7-1790848800000".into(),
                agent: Some("app".into()),
                harness: Some("claude-code".into()),
                model: Some("claude-opus-5-5".into()),
                pane: Some(7),
            }]
        );
        // The run carries no decision out, so the one chant ranks first.
        assert_eq!(
            h.decisions,
            [Decision {
                id: "why-001".into(),
                title: Some("The server answers on one port".into()),
                state: Some("decided".into()),
                relevance: "path".into(),
            }]
        );
        assert_eq!(h.holder, None, "a lease is only said for lines not committed");
        // With its block closed, the run is still named, with no pane.
        let gone = Why::read(&intent, Some(&status), &Panes::default()).unwrap();
        assert_eq!(gone.hunk(&[3]).unwrap().runs[0].pane, None);
        assert_eq!(gone.hunk(&[7]).unwrap().holder.unwrap().pane, None);
    }

    #[test]
    fn an_uncommitted_hunk_says_so_and_who_holds_the_lease() {
        let (intent, status) = docs();
        assert!(Why::uncommitted(&intent));
        let why = Why::read(&intent, Some(&status), &panes()).unwrap();
        let h = why.hunk(&[7]).unwrap();
        assert_eq!((h.committed, h.uncommitted), (0, 1));
        assert!(h.commits.is_empty() && h.runs.is_empty());
        assert_eq!(h.holder, Some(Holder { name: "app".into(), item: "W-001".into(), pane: Some(7) }));
        assert_eq!(h.decisions[0].id, "why-001");
        // Without status, or with the lease gone, nobody holds it.
        let why = Why::read(&intent, None, &panes()).unwrap();
        assert_eq!(why.hunk(&[7]).unwrap().holder, None);
        let mut released = status.clone();
        released["leases"] = json!([]);
        let why = Why::read(&intent, Some(&released), &panes()).unwrap();
        assert_eq!(why.hunk(&[7]).unwrap().holder, None);
        // A lease on another item, in the member's own ledger: not said.
        let mut other = status.clone();
        other["leases"][0]["item"] = json!("W-002");
        other["leases"][0]["member"] = json!("app");
        let why = Why::read(&intent, Some(&other), &panes()).unwrap();
        assert_eq!(why.hunk(&[7]).unwrap().holder, None);
        // A block running as the session that didn't write the run.
        let fresh = Panes::of("/ws", &[(3, json!({ "chant": { "root": "/ws", "agent": "app" } }), json!({}))]);
        let why = Why::read(&intent, Some(&status), &fresh).unwrap();
        assert_eq!(why.hunk(&[3]).unwrap().runs[0].pane, None, "only the block that wrote it");
        assert_eq!(why.hunk(&[7]).unwrap().holder.unwrap().pane, Some(3));
    }

    #[test]
    fn a_removal_has_no_why_and_a_mixed_hunk_has_both() {
        let (intent, status) = docs();
        let why = Why::read(&intent, Some(&status), &panes()).unwrap();
        assert_eq!(why.hunk(&[]), None);
        let h = why.hunk(&[3, 7]).unwrap();
        assert_eq!((h.committed, h.uncommitted), (1, 1));
        assert_eq!(h.runs.len(), 1);
        assert!(h.holder.is_some());
    }

    #[test]
    fn a_carried_decision_comes_before_the_file_s() {
        let (mut intent, status) = docs();
        // As if the run's end had named a second decision it carried out.
        let mut d2 = intent["nodes"].as_array().unwrap().iter().find(|n| n["kind"] == "decision").unwrap().clone();
        d2["id"] = json!("record:decision/why-002");
        d2["record"] = json!("why-002");
        d2["title"] = json!("Ports come from the environment");
        intent["nodes"].as_array_mut().unwrap().push(d2);
        intent["why"]["decisions"].as_array_mut().unwrap().insert(
            0,
            json!({ "decision": "record:decision/why-002", "relevance": "carried", "current": true, "closed": false, "lines": 1 }),
        );
        intent["why"]["runs"][0]["decisions"] = json!(["record:decision/why-002"]);
        let why = Why::read(&intent, Some(&status), &panes()).unwrap();
        let h = why.hunk(&[3]).unwrap();
        assert_eq!(
            h.decisions.iter().map(|d| (d.id.as_str(), d.relevance.as_str())).collect::<Vec<_>>(),
            [("why-002", "carried")]
        );
        // An uncommitted line carries nothing out: the one constraining the file.
        assert_eq!(why.hunk(&[7]).unwrap().decisions[0].id, "why-001");
    }

    #[test]
    fn chant_s_failures_and_an_older_chant_say_why() {
        let e = json!({ "contract": 1, "error": { "code": "intent-region-invalid", "message": "no such path" } });
        assert_eq!(Why::read(&e, None, &Panes::default()).unwrap_err(), "no such path (intent-region-invalid)");
        let old = json!({ "contract": 1, "nodes": [], "edges": [] });
        assert!(Why::read(&old, None, &Panes::default()).unwrap_err().contains("0.102.0"));
    }
}
