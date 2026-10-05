//! A chant workspace read through chant's read contract (`ws-017`) and
//! nothing else (from S21's spike, `spikes/s21-chant-workspace`).
//!
//! Two halves:
//!
//! - [`SCRIPT`] + [`READER`]: one `sh -c` on the block's host (one exec on
//!   a VM). It finds the workspace's own chant, runs the four reads in
//!   parallel with node, and prints one JSON document: what it ran, how long
//!   each took, and what each said.
//! - [`compose`]: pure. Turns that document into the block's state: member
//!   cards, records, the gates waiting, and a headline.

use arugula_proto::{Gate, GateSource};
use serde::Serialize;
use serde_json::Value;

/// `sh -c SCRIPT sh ROOT READER ENV`: the reads, as one JSON document on
/// stdout, whatever happens. It runs with the user's shell environment
/// (#74), so node from mise, nvm or fnm is on PATH as in a pane.
pub const SCRIPT: &str = r#"cd "$1" 2>/dev/null || { printf '{"error":"no such directory: %s"}' "$1"; exit 0; }
command -v node >/dev/null 2>&1 || { printf '{"error":"node is not on PATH on this host, even in your shell'"'"'s environment"}'; exit 0; }
exec node -e "$2" "$1" "$3""#;

/// The reads. `process.argv`: root, env. Which chant: `$CHANT`, else
/// `node_modules/.bin/chant` here or in the nearest parent, else PATH.
pub const READER: &str = r#"
const { execFile } = require("child_process"), fs = require("fs"), path = require("path");
const [root, env] = process.argv.slice(1);
const here = path.resolve(root);
function which() {
  if (process.env.CHANT) return [process.env.CHANT, "env"];
  for (let d = here; ; d = path.dirname(d)) {
    const p = path.join(d, "node_modules/.bin/chant");
    if (fs.existsSync(p)) return [p, d === here ? "workspace" : "above"];
    if (d === path.dirname(d)) break;
  }
  for (const d of (process.env.PATH || "").split(":")) {
    const p = path.join(d, "chant");
    try { fs.accessSync(p, fs.constants.X_OK); return [p, "path"]; } catch {}
  }
  return [null, null];
}
// Here and each parent up to the git root: where chant looks for the
// declaration. Only to say why when there's no chant to ask; with one,
// chant decides (declaration-missing).
function up(found) {
  for (let d = here; ; d = path.dirname(d)) {
    if (found(d)) return d;
    if (fs.existsSync(path.join(d, ".git")) || d === path.dirname(d)) return null;
  }
}
const declared = up((d) => ["chant.workspace.json", "chant.workspace.jsonc"].some((f) => fs.existsSync(path.join(d, f)))) !== null;
const gitRoot = up((d) => fs.existsSync(path.join(d, ".git")));
const [chant, how] = which();
function run(args) {
  const t = Date.now();
  return new Promise((done) => execFile(chant, args, { cwd: root, maxBuffer: 64 << 20, timeout: 25000 }, (e, out, err) => {
    let json = null;
    try { json = JSON.parse(out); } catch {}
    done({ code: e ? (typeof e.code === "number" ? e.code : -1) : 0, ms: Date.now() - t, json, err: String(err).replace(/\x1b\[[0-9;]*m/g, "").trim().slice(-600) });
  }));
}
(async () => {
  const doc = { root: here, declared, gitRoot, chant, how, env };
  if (!chant) return console.log(JSON.stringify(doc));
  const t = Date.now();
  const [ls, check, records, status] = await Promise.all([
    run(["workspace", "ls", "--json"]),
    run(["workspace", "check", "--format", "json"]),
    run(["workspace", "records", "--current", "--json"]),
    run(["workspace", "status", env, "--json"]),
  ]);
  doc.ms = Date.now() - t;
  doc.reads = { ls, check, records, status };
  // A chant older than the contract writes none of it: its version, to
  // say which to install.
  if (![ls, check, records, status].some((r) => r.json && typeof r.json === "object" && "contract" in r.json)) {
    doc.version = await new Promise((done) => execFile(chant, ["--version"], { cwd: root, timeout: 10000 }, (e, out) => {
      const m = /\d+\.\d+\.\d+/.exec(String(out));
      done(m ? m[0] : null);
    }));
  }
  console.log(JSON.stringify(doc));
})();
"#;

/// `sh -c FINGERPRINT sh ROOT`: one line that changes when anything a read
/// would see might have: the `chant/lifecycle` ref (releases and gates)
/// first, as its own word, then a checksum of a commit and the working tree
/// (the declaration, records, member sources). About 0.01 CPU-seconds where
/// a full read is about 7 (four chant processes, each loading chant's
/// TypeScript through tsx), so the block polls this and reads only when it
/// changes. It never fetches.
pub const FINGERPRINT: &str = r#"cd "$1" 2>/dev/null || { echo gone; exit 0; }
printf '%s ' "$(git rev-parse -q --verify refs/heads/chant/lifecycle 2>/dev/null || echo -)"
{ git rev-parse -q --verify HEAD; git status --porcelain=v1; git diff HEAD; } 2>/dev/null | cksum"#;

/// What the block draws and `describe` returns.
#[derive(Debug, Clone, Default, Serialize)]
pub struct State {
    pub root: String,
    pub name: Option<String>,
    /// Which chant answered, and how it was found: `workspace`
    /// (`node_modules/.bin` here), `above` (a parent's), `path`, `env`
    /// (`$CHANT`).
    pub chant: Option<String>,
    pub how: Option<String>,
    pub version: Option<String>,
    /// The environment `status` read gates and releases for.
    pub env: String,
    pub members: Vec<Member>,
    pub records: Vec<Record>,
    /// Why there are no records, when there aren't.
    pub records_note: Option<String>,
    /// Gates waiting for a person, in any member.
    pub gates: Vec<Gate>,
    /// Declaration findings for no member (or the workspace as a whole).
    pub diagnostics: Vec<Diagnostic>,
    /// Each read: how long, and whether it answered.
    pub reads: Vec<Read>,
    /// All of them, in parallel.
    pub ms: u64,
    /// The block can't show the workspace at all, and why.
    pub error: Option<String>,
    /// chant's reason code for it (`declaration-missing`,
    /// `reader-too-old`, ...), or Arugula's own for the contract:
    /// `chant-too-old`, `contract-unknown`.
    pub error_code: Option<String>,
    /// What wants you most, in a line (see [`State::headline`]).
    pub headline: Option<String>,
    pub loading: bool,
    pub updated_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Member {
    pub name: String,
    pub dir: String,
    /// The member's directory on the host, for opening panes there.
    pub path: String,
    pub kind: String,
    pub because: Option<String>,
    pub roles: Vec<String>,
    /// A nested workspace: one card here, its own block when opened.
    pub nested: bool,
    /// Why chant couldn't read it.
    pub unreadable: Option<String>,
    pub errors: usize,
    pub warnings: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub releases: usize,
    pub gates: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Record {
    pub kind: String,
    pub id: String,
    pub title: Option<String>,
    pub state: Option<String>,
    pub ready: Option<bool>,
    pub blocked_by: Vec<String>,
    /// Pin drift and the like.
    pub warnings: Vec<String>,
    pub valid: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Read {
    pub name: String,
    pub ms: u64,
    pub code: i64,
    /// A result came back (not a failure, not something else).
    pub ok: bool,
    /// chant's own words when it didn't.
    pub note: Option<String>,
    /// The failure's reason code, when chant gave one.
    pub reason: Option<String>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn join(root: &str, dir: &str) -> String {
    if dir == "." || dir.is_empty() { root.to_owned() } else { format!("{}/{dir}", root.trim_end_matches('/')) }
}

/// What the block says when there's no chant to read with.
pub const NO_CHANT: &str = "no chant here: not in $CHANT, not in node_modules/.bin here or above (run npm install), \
                            and not on PATH";

/// What it says when the directory isn't in a workspace (and there's no
/// chant to ask).
pub const NO_DECLARATION: &str =
    "no chant.workspace.json or .jsonc here or above it: `chant workspace init` proposes one";

/// The principal chant records for an Arugula name (ws-080): the one the
/// block's `principals` gives it, else the name itself (a forge identity
/// such as `github:alice` already, or a plain name where the workspace
/// takes one).
pub fn principal(name: &str, principals: &std::collections::BTreeMap<String, String>) -> String {
    let name = name.trim();
    principals
        .get(name)
        .or_else(|| principals.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
        .cloned()
        .unwrap_or_else(|| name.to_owned())
}

/// The first chant that takes `chant approve --relayed-by` (chant#3402).
pub const RELAYED_BY: &str = "0.103.1";

/// The read contract this reads (ws-017), and the first chant that writes it.
pub const CONTRACT: u64 = 1;
pub const FLOOR: &str = "0.81.0";

/// The `$id` a read's document names in `$schema`.
fn schema_id(read: &str) -> String {
    format!("https://intentius.io/chant/schemas/workspace/{read}/v1/{read}.schema.json")
}

/// `0.103.1` as numbers, for comparing; a pre-release counts as its release.
pub fn version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// `v` is `floor` or newer.
pub fn at_least(v: &str, floor: &str) -> bool {
    matches!((version(v), version(floor)), (Some(a), Some(b)) if a >= b)
}

/// Why these documents can't be read as contract 1, if they can't: the
/// chant is older than the floor, or a document names another contract or
/// schema. A reader that knows contract 1 refuses any other.
fn contract_problem(reads: &Value, chant: Option<&str>) -> Option<(&'static str, String)> {
    let v = chant.unwrap_or("?");
    if chant.is_some_and(|c| !at_least(c, FLOOR)) {
        return Some((
            "chant-too-old",
            format!(
                "chant {v} is older than {FLOOR}, the first that writes the read contract: install @intentius/chant {FLOOR} or newer"
            ),
        ));
    }
    for name in ["ls", "check", "records", "status"] {
        let doc = &reads[name]["json"];
        if !doc.is_object() {
            continue;
        }
        match doc["contract"].as_u64() {
            Some(CONTRACT) => {}
            Some(n) => {
                return Some((
                    "contract-unknown",
                    format!("chant {v} writes read contract {n}; this Arugula reads contract {CONTRACT}"),
                ));
            }
            None => {
                return Some((
                    "chant-too-old",
                    format!("chant {v}'s `{name}` names no read contract: install @intentius/chant {FLOOR} or newer"),
                ));
            }
        }
        let want = schema_id(name);
        if let Some(other) = doc["$schema"].as_str().filter(|s| *s != want) {
            return Some(("contract-unknown", format!("chant {v}'s `{name}` follows {other}, not {want}")));
        }
    }
    None
}

/// The reader's document as block state.
pub fn compose(raw: &Value, env: &str) -> State {
    let mut st = State { env: env.to_owned(), root: s(&raw["root"]).unwrap_or_default(), ..Default::default() };
    st.chant = s(&raw["chant"]);
    st.how = s(&raw["how"]);
    if let Some(e) = s(&raw["error"]) {
        st.error = Some(e);
        return st.headlined();
    }
    // With a chant, it says whether this is a workspace (it looks above
    // too, for .json and .jsonc); without one, the reader looked the same
    // way.
    if st.chant.is_none() {
        st.error = Some(if raw["declared"] == false { NO_DECLARATION } else { NO_CHANT }.into());
        return st.headlined();
    }
    st.ms = raw["ms"].as_u64().unwrap_or(0);
    let reads = &raw["reads"];
    for name in ["ls", "check", "records", "status"] {
        let r = &reads[name];
        // A failure is `error: {code, message}` (the schemas' `oneOf`).
        let failure = &r["json"]["error"];
        let ok = r["json"].is_object() && !failure.is_object();
        let code = r["code"].as_i64().unwrap_or(-1);
        let reason = s(&failure["code"]);
        let note = if failure.is_object() {
            Some(s(&failure["message"]).unwrap_or_default()).filter(|m| !m.is_empty()).or_else(|| reason.clone())
        } else {
            (!ok || code != 0)
                .then(|| s(&r["err"]).filter(|e| !e.is_empty()))
                .flatten()
                .map(|e| e.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_owned())
        };
        st.reads.push(Read { name: name.into(), ms: r["ms"].as_u64().unwrap_or(0), code, ok, note, reason });
    }
    st.version = ["ls", "status", "check", "records"]
        .iter()
        .find_map(|n| s(&reads[*n]["json"]["chant"]))
        .or_else(|| s(&raw["version"]));
    if let Some((code, why)) = contract_problem(reads, st.version.as_deref()) {
        st.error = Some(why);
        st.error_code = Some(code.into());
        return st.headlined();
    }

    // Members, from `ls`. Without it there's nothing to draw.
    let ls = &reads["ls"]["json"];
    if let Some(r) = st.reads.iter().find(|r| r.name == "ls" && !r.ok) {
        let why = r.note.clone().unwrap_or_else(|| "no answer".into());
        st.error = Some(match &r.reason {
            Some(_) => why,
            None => format!("chant couldn't list the workspace: {why}"),
        });
        st.error_code.clone_from(&r.reason);
        return st.headlined();
    }
    st.name = s(&ls["workspace"]["name"]);
    // The root chant found: `workspace.root` is relative to the git root
    // (absolute outside git), and may be above the directory opened.
    if let Some(r) = s(&ls["workspace"]["root"]) {
        if r.starts_with('/') {
            st.root = r;
        } else if let Some(g) = s(&raw["gitRoot"]) {
            st.root = join(&g, &r);
        }
    }
    let root = st.root.clone();
    for m in ls["members"].as_array().into_iter().flatten() {
        let dir = s(&m["dir"]).unwrap_or_default();
        let kind = s(&m["kind"]).unwrap_or_default();
        st.members.push(Member {
            name: s(&m["name"]).unwrap_or_default(),
            path: join(&root, &dir),
            dir,
            nested: kind == "workspace",
            kind,
            because: s(&m["because"]),
            roles: m["roles"].as_array().into_iter().flatten().filter_map(s).collect(),
            unreadable: (m["readable"] == false)
                .then(|| s(&m["reason"]["message"]).or_else(|| s(&m["reason"]["code"])).unwrap_or_default()),
            ..Default::default()
        });
    }

    // Declaration findings, onto the member they name. `WSP009` ("kind
    // other, which chant does not read") is the declaration working as
    // meant, not a problem: it's the card's `because`.
    let check = &reads["check"]["json"];
    let diags = check["declaration"]["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(check["findings"].as_array().into_iter().flatten());
    for d in diags {
        let rule = s(&d["ruleId"]).or_else(|| s(&d["code"])).unwrap_or_default();
        if rule == "WSP009" {
            continue;
        }
        let diag = Diagnostic {
            severity: s(&d["severity"]).unwrap_or_else(|| "warning".into()),
            message: s(&d["message"]).unwrap_or_default(),
            file: s(&d["file"]),
            line: d["line"].as_u64(),
            rule,
        };
        let on = s(&d["entity"]).or_else(|| s(&d["member"]));
        match st.members.iter_mut().find(|m| Some(&m.name) == on.as_ref()) {
            Some(m) => {
                match diag.severity.as_str() {
                    "error" => m.errors += 1,
                    "warning" => m.warnings += 1,
                    _ => {}
                }
                m.diagnostics.push(diag);
            }
            None => st.diagnostics.push(diag),
        }
    }

    // Releases and pending gates, from `status`.
    let status = &reads["status"]["json"];
    for sm in status["members"].as_array().into_iter().flatten() {
        let name = s(&sm["name"]).unwrap_or_default();
        let Some(m) = st.members.iter_mut().find(|m| m.name == name) else { continue };
        m.releases = sm["environments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| e["releases"].as_array().map_or(0, Vec::len))
            .sum();
        for g in sm["gates"].as_array().into_iter().flatten() {
            if g["state"].as_str().is_some_and(|s| s != "pending") {
                continue;
            }
            m.gates += 1;
            let (op, gate, env) =
                (s(&g["component"]).unwrap_or_default(), s(&g["name"]).unwrap_or_default(), s(&g["env"]));
            // Status's line is what approving runs (#302); a chant that
            // gives none gets the same, from the gate's plan and rule.
            let command = s(&g["approve"]).or_else(|| {
                let mut line = format!("chant approve {op} {gate}");
                if let Some(e) = &env {
                    line += &format!(" --env {e}");
                }
                if let Some(p) = g["planDigest"].as_str() {
                    line += &format!(" --plan {p}");
                }
                if g["signed"].is_object() {
                    line += " --sign";
                }
                Some(line)
            });
            st.gates.push(Gate {
                member: name.clone(),
                op,
                gate,
                env,
                since: s(&g["recordedAt"]),
                expires: s(&g["expiresAt"]),
                approvals: g["approvals"].as_array().map_or(0, |a| a.len() as u64),
                needed: g["needed"].as_u64().unwrap_or(1),
                command,
                source: GateSource::Chant { root: root.clone(), dir: m.path.clone(), machine: None },
            });
        }
    }

    // Records: the current ones of every kind the declaration names.
    let rec = &reads["records"]["json"];
    let kinds: Vec<&Value> = match rec["kinds"].as_array() {
        Some(k) => k.iter().collect(),
        None if !rec.is_null() => vec![rec],
        None => vec![],
    };
    if kinds.is_empty() {
        st.records_note = Some(
            st.reads
                .iter()
                .find(|r| r.name == "records")
                .and_then(|r| r.note.clone().map(|n| r.reason.as_ref().map_or(n.clone(), |c| format!("{n} ({c})"))))
                .unwrap_or_else(|| "no record kinds".into()),
        );
    }
    for k in kinds {
        let kind =
            s(&k["kind"]["name"]).or_else(|| s(&k["kind"])).or_else(|| s(&k["declared"]["name"])).unwrap_or_default();
        for r in k["records"].as_array().into_iter().flatten() {
            st.records.push(Record {
                kind: kind.clone(),
                id: s(&r["id"]).unwrap_or_default(),
                title: s(&r["data"]["title"]),
                state: s(&r["state"]),
                ready: r["ready"].as_bool(),
                blocked_by: r["blockedBy"].as_array().into_iter().flatten().filter_map(|b| s(&b["id"])).collect(),
                warnings: r["warnings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| s(&w["message"]).or_else(|| s(&w["code"])).or_else(|| s(w)))
                    .collect(),
                valid: r["valid"].as_bool().unwrap_or(true),
            });
        }
    }
    st.headlined()
}

impl State {
    fn headlined(mut self) -> Self {
        self.headline = self.headline();
        self
    }

    /// What wants you most, if anything: an error, a gate waiting (with
    /// how many more), a member chant can't read or with errors, a record
    /// whose pin drifted. Only a gate is attention (see the block); the
    /// rest is for the block's own bar.
    pub fn headline(&self) -> Option<String> {
        if let Some(e) = &self.error {
            return Some(e.clone());
        }
        if let Some(g) = self.gates.first() {
            return Some(gates_headline(g, self.gates.len()));
        }
        if let Some(m) = self.members.iter().find(|m| m.unreadable.is_some() || m.errors > 0) {
            return Some(format!(
                "{}: {}",
                m.name,
                m.unreadable.clone().unwrap_or_else(|| format!("{} errors", m.errors))
            ));
        }
        self.records.iter().find(|r| !r.warnings.is_empty()).map(|r| format!("{}: {}", r.id, r.warnings[0]))
    }

    /// For `capture --text`, history and search.
    pub fn text(&self) -> String {
        let mut out = format!(
            "{} ({})  chant {} via {}  {} ms\n",
            self.name.as_deref().unwrap_or("workspace"),
            self.root,
            self.version.as_deref().unwrap_or("?"),
            self.how.as_deref().unwrap_or("-"),
            self.ms
        );
        if let Some(e) = &self.error {
            out += &format!("\n  ! {e}{}\n", self.error_code.as_ref().map(|c| format!(" ({c})")).unwrap_or_default());
            return out;
        }
        if !self.gates.is_empty() {
            out += "\nwaiting on you\n";
            for g in &self.gates {
                out += &format!(
                    "  {}/{} gate {}  ({}/{})  {}\n",
                    g.member,
                    g.op,
                    g.gate,
                    g.approvals,
                    g.needed,
                    g.command.as_deref().unwrap_or("")
                );
            }
        }
        out += &format!("\nmembers ({})\n", self.members.len());
        for m in &self.members {
            let mut tags = vec![m.kind.clone()];
            if m.errors > 0 {
                tags.push(format!("{} errors", m.errors));
            }
            if m.warnings > 0 {
                tags.push(format!("{} warnings", m.warnings));
            }
            if m.gates > 0 {
                tags.push(format!("{} gates", m.gates));
            }
            if m.releases > 0 {
                tags.push(format!("{} releases", m.releases));
            }
            if let Some(u) = &m.unreadable {
                tags.push(format!("unreadable: {u}"));
            }
            out += &format!("  {:<28} {:<36} {}\n", m.name, m.dir, tags.join(", "));
        }
        if !self.records.is_empty() {
            out += &format!("\nrecords ({})\n", self.records.len());
            for r in &self.records {
                let blocked = if r.blocked_by.is_empty() {
                    String::new()
                } else {
                    format!("  blocked by {}", r.blocked_by.join(", "))
                };
                out += &format!(
                    "  {:<10} {:<12} {}{blocked}\n",
                    r.id,
                    r.state.as_deref().unwrap_or("-"),
                    r.title.as_deref().unwrap_or("")
                );
                for w in &r.warnings {
                    out += &format!("             ! {w}\n");
                }
            }
        } else if let Some(n) = &self.records_note {
            out += &format!("\nrecords: {n}\n");
        }
        out += "\nreads:";
        for r in &self.reads {
            out += &format!(" {} {}ms{}", r.name, r.ms, if r.ok { "" } else { " (failed)" });
        }
        out.push('\n');
        out
    }
}

/// The first gate's headline, and how many more wait.
pub fn gates_headline(first: &Gate, n: usize) -> String {
    let more = if n > 1 { format!(" (+{} more)", n - 1) } else { String::new() };
    format!("{}{more}", first.headline())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_reference_workspace() {
        let st = compose(&fixture(include_str!("fixtures/reference-raw.json")), "local");
        assert_eq!(st.error, None);
        assert_eq!(st.name.as_deref(), Some("reference"));
        assert_eq!(
            st.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["app", "delivery", "design-client", "design"]
        );
        let delivery = st.members.iter().find(|m| m.name == "delivery").unwrap();
        assert_eq!(
            (delivery.kind.as_str(), delivery.gates, delivery.path.as_str()),
            ("chant", 1, "/scratch/refws/delivery")
        );
        // WSP009 (kind other) is the card's `because`, not a warning.
        let app = st.members.iter().find(|m| m.name == "app").unwrap();
        assert_eq!(app.warnings, 0);
        assert!(app.because.as_deref().unwrap().starts_with("a Node HTTP server"));
        // The approved gate isn't waiting; the new one is.
        assert_eq!(st.gates.len(), 1);
        let g = &st.gates[0];
        assert_eq!((g.op.as_str(), g.gate.as_str(), g.approvals, g.needed), ("release", "approve-release", 0, 1));
        assert_eq!(g.command.as_deref(), Some("chant approve release approve-release"));
        assert_eq!(
            g.source,
            GateSource::Chant { root: "/scratch/refws".into(), dir: "/scratch/refws/delivery".into(), machine: None }
        );
        assert_eq!(g.key(), "delivery/release/approve-release");
        assert_eq!(g.bundle(), "gate:/scratch/refws");
        assert_eq!(st.headline.as_deref(), Some("delivery: release waits at gate approve-release"));
        let w2 = st.records.iter().find(|r| r.id == "W-002").unwrap();
        assert_eq!(w2.blocked_by, ["W-001"]);
        assert!(st.reads.iter().all(|r| r.ok));
        assert!(st.text().contains("delivery/release gate approve-release"));
    }

    #[test]
    fn more_gates_say_how_many() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        // Both of delivery's gates pending.
        for m in raw["reads"]["status"]["json"]["members"].as_array_mut().unwrap() {
            for g in m["gates"].as_array_mut().unwrap() {
                g["state"] = "pending".into();
            }
        }
        let st = compose(&raw, "local");
        assert_eq!(st.gates.len(), 2);
        assert_eq!(st.members.iter().find(|m| m.name == "delivery").unwrap().gates, 2);
        assert_eq!(st.headline.as_deref(), Some("delivery: release waits at gate approve-release (+1 more)"));
    }

    #[test]
    fn no_chant_installed() {
        let st = compose(&fixture(include_str!("fixtures/no-chant-raw.json")), "local");
        assert_eq!(st.error.as_deref(), Some(NO_CHANT));
        assert_eq!(st.headline.as_deref(), Some(NO_CHANT));
        assert!(st.members.is_empty() && st.gates.is_empty());
    }

    #[test]
    fn not_a_workspace() {
        let st = compose(&serde_json::json!({ "root": "/tmp/x", "declared": false, "chant": null }), "local");
        assert_eq!(st.error.as_deref(), Some(NO_DECLARATION));
        // With a chant, chant says (declaration-missing): the reader's own
        // look doesn't decide.
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["declared"] = false.into();
        assert_eq!(compose(&raw, "local").error, None);
    }

    #[test]
    fn chant_failing_to_list() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["ls"] =
            serde_json::json!({ "code": 1, "ms": 5, "json": null, "err": "\nWSP001: bad declaration\nmore" });
        let st = compose(&raw, "local");
        assert_eq!(st.error.as_deref(), Some("chant couldn't list the workspace: WSP001: bad declaration"));
        let ls = st.reads.iter().find(|r| r.name == "ls").unwrap();
        assert!(!ls.ok);
    }

    /// A failure in `ls` as chant prints it (exit 1).
    fn ls_failure(code: &str, message: &str) -> Value {
        serde_json::json!({ "code": 1, "ms": 5, "err": "", "json": {
            "$schema": "https://intentius.io/chant/schemas/workspace/ls/v1/ls.schema.json",
            "contract": 1, "chant": "0.95.0", "error": { "code": code, "message": message, "location": null },
        } })
    }

    #[test]
    fn a_failure_is_chants_reason_not_an_empty_workspace() {
        for code in ["declaration-missing", "reader-too-old", "root-chant-required"] {
            let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
            raw["reads"]["ls"] = ls_failure(code, "chant says why");
            let st = compose(&raw, "local");
            assert_eq!(st.error.as_deref(), Some("chant says why"), "{code}");
            assert_eq!(st.error_code.as_deref(), Some(code));
            assert!(st.members.is_empty() && st.gates.is_empty());
            let ls = st.reads.iter().find(|r| r.name == "ls").unwrap();
            assert!(!ls.ok);
            assert_eq!(ls.reason.as_deref(), Some(code));
            assert!(st.text().contains(&format!("chant says why ({code})")));
        }
    }

    #[test]
    fn a_records_failure_says_its_code() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["records"]["json"] = serde_json::json!({
            "$schema": "https://intentius.io/chant/schemas/workspace/records/v1/records.schema.json",
            "contract": 1, "error": { "code": "kind-unreadable", "message": "work.kind.mjs threw" },
        });
        let st = compose(&raw, "local");
        assert_eq!(st.error, None);
        assert!(st.records.is_empty());
        assert_eq!(st.records_note.as_deref(), Some("work.kind.mjs threw (kind-unreadable)"));
    }

    #[test]
    fn another_contract_is_refused() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["status"]["json"]["contract"] = 2.into();
        let st = compose(&raw, "local");
        assert_eq!(st.error_code.as_deref(), Some("contract-unknown"));
        assert!(st.error.as_deref().unwrap().contains("read contract 2"), "{:?}", st.error);
        assert!(st.members.is_empty() && st.gates.is_empty());

        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["ls"]["json"]["$schema"] =
            "https://intentius.io/chant/schemas/workspace/ls/v2/ls.schema.json".into();
        let st = compose(&raw, "local");
        assert_eq!(st.error_code.as_deref(), Some("contract-unknown"));
        assert!(st.error.as_deref().unwrap().contains("/ls/v2/"));
    }

    #[test]
    fn a_chant_older_than_the_floor_says_what_to_install() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        for n in ["ls", "check", "records", "status"] {
            raw["reads"][n] =
                serde_json::json!({ "code": 1, "ms": 5, "json": null, "err": "unknown command workspace" });
        }
        raw["version"] = "0.79.2".into();
        let st = compose(&raw, "local");
        assert_eq!(st.version.as_deref(), Some("0.79.2"));
        assert_eq!(st.error_code.as_deref(), Some("chant-too-old"));
        assert!(st.error.as_deref().unwrap().contains("install @intentius/chant 0.81.0 or newer"));
        // ...and one that writes JSON but no contract.
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        for n in ["ls", "check", "records", "status"] {
            raw["reads"][n]["json"].as_object_mut().unwrap().remove("contract");
            raw["reads"][n]["json"].as_object_mut().unwrap().remove("chant");
        }
        let st = compose(&raw, "local");
        assert_eq!(st.error_code.as_deref(), Some("chant-too-old"));
    }

    #[test]
    fn a_gate_without_a_line_gets_one_with_its_plan_and_rule() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        let g = &mut raw["reads"]["status"]["json"]["members"][1]["gates"][0];
        g.as_object_mut().unwrap().remove("approve");
        g["planDigest"] = "sha256:ab12".into();
        g["env"] = "prod".into();
        g["signed"] = serde_json::json!({ "class": null });
        let st = compose(&raw, "local");
        assert_eq!(
            st.gates[0].command.as_deref(),
            Some("chant approve release approve-release --env prod --plan sha256:ab12 --sign")
        );
    }

    #[test]
    fn principals() {
        let map: std::collections::BTreeMap<String, String> =
            [("friend@example.com".to_owned(), "github:friend".to_owned())].into();
        assert_eq!(principal("friend@example.com", &map), "github:friend");
        assert_eq!(principal("Friend@Example.com", &map), "github:friend");
        assert_eq!(principal("github:alice", &map), "github:alice");
        assert_eq!(principal(" alex ", &map), "alex");
    }

    #[test]
    fn versions() {
        assert!(at_least("0.81.0", FLOOR) && at_least("0.103.1", "0.103.1") && at_least("1.0.0-rc.1", "0.103.1"));
        assert!(!at_least("0.80.9", FLOOR) && !at_least("0.103.0", "0.103.1") && !at_least("?", FLOOR));
        assert_eq!(version("v0.95.0"), Some((0, 95, 0)));
    }

    #[test]
    fn chant_finds_the_declaration_above() {
        // Opened in a member's directory; chant found the workspace at the
        // git root's `ws` (its `workspace.root`).
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["root"] = "/repo/ws/delivery".into();
        raw["gitRoot"] = "/repo".into();
        raw["reads"]["ls"]["json"]["workspace"]["root"] = "ws".into();
        raw["reads"]["ls"]["json"]["workspace"]["file"] = "chant.workspace.jsonc".into();
        let st = compose(&raw, "local");
        assert_eq!(st.error, None);
        assert_eq!(st.root, "/repo/ws");
        let delivery = st.members.iter().find(|m| m.name == "delivery").unwrap();
        assert_eq!(delivery.path, "/repo/ws/delivery");
        assert_eq!(st.gates[0].bundle(), "gate:/repo/ws");
        // At the git root itself.
        raw["reads"]["ls"]["json"]["workspace"]["root"] = ".".into();
        assert_eq!(compose(&raw, "local").root, "/repo");
    }

    #[test]
    fn a_nested_workspace_is_a_card() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["ls"]["json"]["members"][0]["kind"] = "workspace".into();
        let st = compose(&raw, "local");
        assert!(st.members[0].nested);
    }

    #[test]
    fn the_script_takes_its_arguments_and_no_shell_of_its_own() {
        assert!(SCRIPT.starts_with(r#"cd "$1""#) && SCRIPT.ends_with(r#""$1" "$3""#));
        assert!(!SCRIPT.contains("-ic"));
    }
}
