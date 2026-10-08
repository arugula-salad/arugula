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

use arugula_proto::{
    Gate, GateSource,
    workspace::{
        DecisionRef, GateRelease, GateWhy, Lease, MemberWhy, PinState, Record, RecordChoice, RecordEvidence, RunRef,
    },
};
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
///
/// That only finds a chant to start (#305, ws-021). Which chant reads the
/// declaration stays chant's call: one the root doesn't pin hands the
/// command line to the pinned one installed at the root, or refuses with
/// `root-chant-required`, and an unpinned root is read by whichever chant
/// meets `minReader` (`reader-too-old` otherwise). The block shows those
/// codes and which chant it started (`how`).
///
/// The declaration's `agents` (#304) are read from its file, since no read
/// prints them: `null` when it isn't plain JSON.
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
const declDir = up((d) => ["chant.workspace.json", "chant.workspace.jsonc"].some((f) => fs.existsSync(path.join(d, f))));
const declared = declDir !== null;
const gitRoot = up((d) => fs.existsSync(path.join(d, ".git")));
// The agent sessions (ws-067), which no read prints: from the declaration
// (a .jsonc one gives none).
let agents = null;
try { const a = JSON.parse(fs.readFileSync(path.join(declDir, "chant.workspace.json"), "utf8")).agents; agents = Array.isArray(a) ? a : []; } catch {}
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
  const doc = { root: here, declared, gitRoot, chant, how, env, agents };
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
/// first, as its own word, then `HEAD`, then a checksum of a commit, chant's
/// other refs and the working tree (the declaration, records, member
/// sources). The first two words are what an intent read depends on
/// (#617): its history and the committed records, not the working tree. About
/// 0.01 CPU-seconds where a full read is about 7 (four chant processes,
/// each loading chant's TypeScript through tsx), so the block polls this
/// and reads only when it changes. It never fetches.
///
/// #305: `GIT_OPTIONAL_LOCKS=0` (git's `--no-optional-locks`) keeps
/// `status` from refreshing the index and taking `index.lock` every few
/// seconds, which would fail a person's `git commit` or chant's own writes
/// that land at the same moment. chant's other refs are in the checksum
/// because `status` reports them: leases (`refs/chant/lease/*`, its
/// `leases`), work in progress, kept attempts and work branches
/// (`refs/chant/wip/*`, `kept/*`, `refs/heads/chant/work/*`, its
/// `replication`). A lease that expires by the clock moves no ref, so it
/// shows at the next read for another reason (its card says `expired` then).
/// The diff is `diff-index` (plumbing): porcelain `git diff HEAD` refreshes
/// and rewrites the index even with `GIT_OPTIONAL_LOCKS=0`.
///
/// Untracked files: `status` shows each only as its `??` line, and the diff
/// leaves them out, so an edit inside one would move nothing. Their mtimes
/// and sizes are in the checksum too (`ls-files -o`, which reads the index
/// and never writes it, then `stat`: GNU/busybox `-c` with nanosecond
/// mtimes, BSD `-f` on macOS, in one `xargs`), not their contents. Capped
/// at the first 1000 files, so a large untracked tree (a build output
/// nobody ignored) costs a bounded `stat`; an edit past the cap shows at
/// the next read for another reason. The walk itself is the one `status`
/// already does.
pub const FINGERPRINT: &str = r#"cd "$1" 2>/dev/null || { echo gone; exit 0; }
export GIT_OPTIONAL_LOCKS=0
printf '%s ' "$(git rev-parse -q --verify refs/heads/chant/lifecycle 2>/dev/null || echo -)"
printf '%s ' "$(git rev-parse -q --verify HEAD 2>/dev/null || echo -)"
if stat -c %s . >/dev/null 2>&1; then set -- -c '%y %s %n'; else set -- -f '%Fm %z %N'; fi
{ git rev-parse -q --verify HEAD; git for-each-ref --format='%(objectname) %(refname)' refs/chant refs/heads/chant/work; git status --porcelain=v1; git diff-index -p HEAD --
git ls-files -o --exclude-standard -z | tr '\0' '\n' | head -n 1000 | tr '\n' '\0' | xargs -0 stat "$@"; } 2>/dev/null | cksum"#;

/// `sh -c LIFECYCLE sh ROOT`: just [`FINGERPRINT`]'s first word, the
/// `chant/lifecycle` ref (one `git rev-parse`), for the block's quick look
/// between full fingerprints.
pub const LIFECYCLE: &str = r#"cd "$1" 2>/dev/null || { echo gone; exit 0; }
git rev-parse -q --verify refs/heads/chant/lifecycle 2>/dev/null || echo -"#;

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
    /// Every work lease `status` lists (#618).
    pub leases: Vec<Lease>,
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
    /// The agent sessions the declaration binds to it (ws-067): an agent
    /// started here runs as the first (#304).
    pub agents: Vec<String>,
    /// The ops `status` names for it, for *Run op* (#309): its stewards'
    /// ops, then any op a gate of its was recorded for. *Run op* takes a
    /// typed name too, for an op `status` doesn't name (chant before
    /// stewards names only gated ones).
    pub ops: Vec<String>,
    /// Work leases on it (#618): held in its ledger, and, once its intent
    /// is read, on work items whose constrains cover it.
    pub leases: Vec<Lease>,
    /// Its decisions, undecided commits and runs, once read (#618).
    pub why: Option<MemberWhy>,
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
    for a in raw["agents"].as_array().into_iter().flatten() {
        let (Some(name), Some(on)) = (s(&a["name"]), s(&a["member"])) else { continue };
        if let Some(m) = st.members.iter_mut().find(|m| m.name == on) {
            m.agents.push(name);
        }
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
        let stewards = sm["stewards"].as_array().into_iter().flatten();
        let ops = stewards.flat_map(|st| st["ops"].as_array().into_iter().flatten().filter_map(|o| s(&o["name"])));
        let gated = sm["gates"].as_array().into_iter().flatten().filter_map(|g| s(&g["component"]));
        for op in ops.chain(gated) {
            if !op.is_empty() && !m.ops.contains(&op) {
                m.ops.push(op);
            }
        }
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
                // The decisions come from another read, when a gate is
                // raised ([`intent`]).
                why: Some(GateWhy {
                    plan_digest: s(&g["planDigest"]),
                    last_release: last_release(sm, &g["component"]),
                    ..Default::default()
                }),
            });
        }
    }

    // Work leases, onto the member whose ledger holds each.
    st.leases = status["leases"].as_array().into_iter().flatten().map(lease).collect();
    for m in &mut st.members {
        m.leases = st.leases.iter().filter(|l| l.member.as_deref() == Some(&m.name)).cloned().collect();
    }

    // Records: the current ones of every kind the declaration names.
    let rec = &reads["records"]["json"];
    let kinds: Vec<&Value> = match rec["kinds"].as_array() {
        Some(k) => k.iter().collect(),
        // A failure (`error: {code, message}`) is no kind.
        None if rec.is_object() && !rec["error"].is_object() => vec![rec],
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
            st.records.push(record(&kind, r));
        }
    }
    st.headlined()
}

/// A member's latest release in the env `status` read (`status` lists the
/// latest of each component): the gate's own op's when it has one, else
/// the newest.
fn last_release(member: &Value, op: &Value) -> Option<GateRelease> {
    let all: Vec<&Value> = member["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|e| e["releases"].as_array().into_iter().flatten())
        .collect();
    let r = all
        .iter()
        .find(|r| r["component"] == *op)
        .or_else(|| all.iter().max_by_key(|r| r["timestamp"].as_str().unwrap_or("")))?;
    Some(GateRelease {
        component: s(&r["component"]).unwrap_or_default(),
        at: s(&r["timestamp"]).unwrap_or_default(),
        actor: s(&r["actor"]),
        git_sha: s(&r["gitSha"]),
    })
}

/// `sh -c INTENT sh ROOT CHANT DIR`: chant's intent graph over a member's
/// directory (`graph --intent`, chant 0.102 and newer), the decisions
/// covering it ranked by `why`. It runs `git log` over the directory, so
/// it's read only when a gate is raised or a member card is opened, and
/// kept until the fingerprint moves (#617).
pub const INTENT: &str = r#"cd "$1" 2>/dev/null || exit 0
exec "$2" workspace graph --intent "$3" --json"#;

/// The first chant whose intent read has `why`.
pub const INTENT_FLOOR: &str = "0.102.0";

/// What a member's intent read says, for its gate and card.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Intent {
    /// Most relevant first (`why.decisions`), current before superseded.
    pub decisions: Vec<DecisionRef>,
    /// Commits that changed the member while no decision constrained it
    /// (`intent-commit-undecided`).
    pub undecided: usize,
    /// The agent runs that made commits there, newest first.
    pub runs: Vec<String>,
    /// The work items whose constrains cover it.
    pub work: Vec<String>,
    /// How long chant took.
    pub ms: u64,
}

/// A member's intent document as [`Intent`], or chant's reason it isn't one.
pub fn intent(doc: &Value, ms: u64) -> Result<Intent, String> {
    if !doc.is_object() {
        return Err("chant said nothing readable about the decisions".into());
    }
    if let Some(e) = doc["error"].as_object() {
        let m = e.get("message").and_then(Value::as_str).unwrap_or_default();
        let c = e.get("code").and_then(Value::as_str).unwrap_or_default();
        return Err(if m.is_empty() { c.to_owned() } else { format!("{m} ({c})") });
    }
    match doc["contract"].as_u64() {
        Some(CONTRACT) => {}
        n => return Err(format!("the intent read names contract {n:?}; this Arugula reads contract {CONTRACT}")),
    }
    let want = schema_id("intent");
    if let Some(other) = doc["$schema"].as_str().filter(|s| *s != want) {
        return Err(format!("the intent read follows {other}, not {want}"));
    }
    let nodes = doc["nodes"].as_array().map(Vec::as_slice).unwrap_or_default();
    let node = |id: &str| nodes.iter().find(|n| n["id"] == id);
    let of_kind = |k: &'static str| nodes.iter().filter(move |n| n["kind"] == k);
    let decision = |n: &Value, relevance: String, current: bool| DecisionRef {
        id: s(&n["record"]).unwrap_or_default(),
        title: s(&n["title"]),
        state: s(&n["state"]),
        relevance,
        current,
        closed: n["closed"].as_bool().unwrap_or(false),
    };
    // chant's own ranking; a document without `why` (before 0.102) in the
    // graph's order, current first.
    let decisions = match doc["why"]["decisions"].as_array() {
        Some(ranked) => ranked
            .iter()
            .filter_map(|w| {
                let n = node(w["decision"].as_str()?)?;
                Some(decision(n, s(&w["relevance"]).unwrap_or_default(), w["current"].as_bool().unwrap_or(true)))
            })
            .collect(),
        None => {
            let mut all: Vec<DecisionRef> = of_kind("decision")
                .map(|n| {
                    let current = n["supersededBy"].is_null();
                    let by = n["constrains"].as_array().and_then(|c| c.first()).and_then(|c| s(&c["granularity"]));
                    decision(n, by.unwrap_or_else(|| "related".into()), current)
                })
                .collect();
            all.sort_by_key(|d| !d.current);
            all
        }
    };
    Ok(Intent {
        decisions,
        undecided: of_kind("finding").filter(|f| f["code"] == "intent-commit-undecided").count(),
        runs: of_kind("run").filter_map(|r| s(&r["run"])).collect(),
        work: of_kind("work").filter_map(|w| s(&w["record"])).collect(),
        ms,
    })
}

/// One of `status`'s leases.
fn lease(l: &Value) -> Lease {
    Lease {
        item: s(&l["item"]).unwrap_or_default(),
        holder: s(&l["holder"]).unwrap_or_default(),
        state: s(&l["state"]).unwrap_or_default(),
        expires_at: s(&l["expiresAt"]),
        member: s(&l["member"]),
        token: s(&l["token"]),
        pane: None,
    }
}

/// `sh -c RUNS sh ROOT CHANT`: the run ledger (`chant workspace runs`).
pub const RUNS: &str = r#"cd "$1" 2>/dev/null || exit 0
exec "$2" workspace runs --json"#;

/// The runs document's runs, newest first, or chant's reason it has none.
pub fn runs(doc: &Value) -> Result<Vec<RunRef>, String> {
    if !doc.is_object() {
        return Err("chant said nothing readable about the runs".into());
    }
    if let Some(e) = doc["error"].as_object() {
        let m = e.get("message").and_then(Value::as_str).unwrap_or_default();
        return Err(if m.is_empty() { "chant couldn't read the runs".into() } else { m.to_owned() });
    }
    if doc["contract"].as_u64() != Some(CONTRACT) {
        return Err(format!(
            "the runs read names contract {}; this Arugula reads contract {CONTRACT}",
            doc["contract"]
        ));
    }
    Ok(doc["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let id = s(&r["id"]).unwrap_or_default();
            RunRef {
                pane: RunRef::pane_of(&id),
                state: s(&r["state"]),
                agent: s(&r["agent"]),
                by: s(&r["by"]),
                started_at: s(&r["startedAt"]),
                ended_at: s(&r["endedAt"]),
                outcome: s(&r["outcome"]),
                unit: s(&r["unit"]["id"]),
                lease: s(&r["lease"]),
                decisions: ids(&r["decisions"], "id"),
                id,
            }
        })
        .collect())
}

/// How many recent runs a member card shows.
pub const RECENT_RUNS: usize = 5;

/// A member's why from its intent read and the run ledger: the decisions
/// chant ranked, its recent runs (its agent sessions', and those chant's
/// walk joined to its commits), and its leases (held in its ledger, or on
/// a work item covering it), each linked to the Arugula pane behind it.
pub fn member_why(
    m: &mut Member,
    all_leases: &[Lease],
    intent: Option<&Result<Intent, String>>,
    ledger: Option<&Result<Vec<RunRef>, String>>,
) {
    let mut why = MemberWhy::default();
    let work: &[String] = match intent {
        Some(Ok(i)) => {
            why.decisions = Some(i.decisions.clone());
            why.undecided = Some(i.undecided as u64);
            why.ms = Some(i.ms);
            &i.work
        }
        Some(Err(e)) => {
            why.note = Some(e.clone());
            &[]
        }
        None => &[],
    };
    let joined: &[String] = match intent {
        Some(Ok(i)) => &i.runs,
        _ => &[],
    };
    let all: &[RunRef] = match ledger {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            why.runs_note = Some(e.clone());
            &[]
        }
        None => &[],
    };
    why.runs = all
        .iter()
        .filter(|r| r.agent.as_ref().is_some_and(|a| m.agents.contains(a)) || joined.contains(&r.id))
        .take(RECENT_RUNS)
        .cloned()
        .collect();
    m.leases = all_leases
        .iter()
        .filter(|l| l.member.as_deref() == Some(&m.name) || work.contains(&l.item))
        .cloned()
        .map(|mut l| {
            // The run under its token; else, while running, one whose agent
            // session holds it.
            l.pane = all
                .iter()
                .find(|r| r.lease.is_some() && r.lease == l.token)
                .or_else(|| {
                    all.iter().find(|r| r.state.as_deref() == Some("running") && r.agent.as_deref() == Some(&l.holder))
                })
                .and_then(|r| r.pane);
            l
        })
        .collect();
    m.why = Some(why);
}

/// The strings in a list, or in each object's `key` (a list of either).
fn ids(v: &Value, key: &str) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| s(x).or_else(|| s(&x[key]))).collect()
}

/// One record of `records --current` (#616): every field a decision can be
/// drawn with, as chant wrote it. Reads only the record itself: its data,
/// and what chant says of it beside the data (`supersededBy`, `assets`,
/// `attested`). A kind whose records name these fields otherwise gets
/// what's there.
fn record(kind: &str, r: &Value) -> Record {
    let d = &r["data"];
    // An option by its id, with the label the record's options give it.
    let label =
        |id: &str| d["options"].as_array().into_iter().flatten().find(|o| o["id"] == id).and_then(|o| s(&o["label"]));
    let choice = |c: &Value, why: &str| {
        let option = s(&c["option"])?;
        Some(RecordChoice { label: label(&option), why: s(&c[why]), option })
    };
    // A pinned file's state, from chant's check of the record's pins.
    let pin = |path: &str| {
        r["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["path"] == path)
            .and_then(|a| PinState::parse(a["state"].as_str()?))
    };
    Record {
        kind: kind.to_owned(),
        id: s(&r["id"]).unwrap_or_default(),
        title: s(&d["title"]),
        state: s(&r["state"]),
        ready: r["ready"].as_bool(),
        blocked_by: ids(&r["blockedBy"], "id"),
        warnings: r["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| s(&w["message"]).or_else(|| s(&w["code"])).or_else(|| s(w)))
            .collect(),
        valid: r["valid"].as_bool().unwrap_or(true),
        question: s(&d["question"]),
        constrains: ids(&d["constrains"], "entry"),
        choice: choice(&d["choice"], "reason"),
        rejected: d["rejected"].as_array().into_iter().flatten().filter_map(|c| choice(c, "why")).collect(),
        // `{decision: id}`, or `{revision, option}` for a choice made before
        // decision files: only the first names a record.
        supersedes: ids(&d["supersedes"], "decision"),
        superseded_by: s(&r["supersededBy"]),
        remediated_by: ids(&r["remediatedBy"], "id"),
        implements: ids(&r["implements"], "id"),
        decided_by: s(&d["decided_by"]),
        decided_on: s(&d["decided_on"]),
        proposed_by: s(&d["proposed_by"]),
        evidence: d["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let path = s(&e["path"]);
                RecordEvidence { title: s(&e["title"]), url: s(&e["url"]), pin: path.as_deref().and_then(pin), path }
            })
            .collect(),
        attested: r["attested"].as_bool(),
        provenance: s(&r["provenance"]["level"]),
        path: s(&r["path"]),
    }
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
                if let Some(why) = &g.why {
                    for d in why.decisions.iter().flatten().filter(|d| d.governs()) {
                        out += &format!("    enforces {} {}\n", d.id, d.title.as_deref().unwrap_or(""));
                    }
                    if let Some(n) = &why.note {
                        out += &format!("    decisions: {n}\n");
                    }
                    if let Some(p) = &why.plan_digest {
                        out += &format!("    plan {p}\n");
                    }
                    if let Some(r) = &why.last_release {
                        out += &format!("    last release {} at {}\n", r.component, r.at);
                    }
                }
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
            for l in &m.leases {
                out += &format!("    lease {} held by {} ({})\n", l.item, l.holder, l.state);
            }
            if let Some(w) = &m.why {
                for d in w.decisions.iter().flatten().filter(|d| d.current) {
                    out += &format!(
                        "    {} {} {}\n",
                        d.id,
                        d.state.as_deref().unwrap_or("-"),
                        d.title.as_deref().unwrap_or("")
                    );
                }
                if let Some(n) = w.undecided.filter(|n| *n > 0) {
                    out += &format!("    {n} commits no decision covers\n");
                }
                for r in &w.runs {
                    out += &format!("    run {} {}\n", r.id, r.state.as_deref().unwrap_or(""));
                }
            }
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
    fn a_decision_keeps_its_fields() {
        // #616: what the reference workspace's decisions say reaches the
        // client, not just id, state and title.
        let st = compose(&fixture(include_str!("fixtures/reference-raw.json")), "local");
        let r = |id: &str| st.records.iter().find(|r| r.id == id).unwrap().clone();
        let d = r("ref-001");
        assert_eq!(d.kind, "decision");
        assert_eq!(d.question.as_deref(), Some("What builds the app's image and runs it?"));
        assert_eq!(d.constrains, ["member:app", "member:delivery"]);
        let choice = d.choice.unwrap();
        assert_eq!(
            (choice.option.as_str(), choice.label.as_deref()),
            ("a", Some("a chant project with the docker lexicon"))
        );
        assert!(choice.why.unwrap().starts_with("The reference workspace needs a `chant` member"));
        assert_eq!(d.rejected.iter().map(|c| c.option.as_str()).collect::<Vec<_>>(), ["b", "c"]);
        assert_eq!(d.rejected[0].label.as_deref(), Some("a hand-written compose file in the app"));
        assert!(d.rejected[0].why.as_deref().unwrap().contains("without a `chant` member"));
        assert_eq!((d.decided_by.as_deref(), d.decided_on.as_deref()), (Some("lex00"), Some("2026-09-23")));
        assert!(d.supersedes.is_empty() && d.superseded_by.is_none());
        assert_eq!((d.attested, d.provenance.as_deref()), (None, Some("unattested")));
        assert_eq!(d.path.as_deref(), Some("decisions/ref-001-how-the-app-is-deployed.md"));
        // Links are evidence with a url and no pin.
        assert_eq!(d.evidence.len(), 2);
        assert!(d.evidence.iter().all(|e| e.url.is_some() && e.path.is_none() && e.pin.is_none()));
        // A pinned file carries chant's word on it.
        let pinned = r("ref-002").evidence.into_iter().find(|e| e.path.is_some()).unwrap();
        assert_eq!(
            (pinned.title.as_deref(), pinned.path.as_deref(), pinned.pin),
            (Some("The home screen spec"), Some("design/screens/home.json"), Some(PinState::Pinned))
        );
        // A work item: the decisions it carries out.
        assert_eq!(r("W-001").implements, ["ref-002"]);
        assert_eq!(r("W-002").blocked_by, ["W-001"]);
        // ...and the wire carries them, by the names the client reads.
        let wire = serde_json::to_value(&st).unwrap();
        let first = &wire["records"][0];
        assert_eq!(first["constrains"], serde_json::json!(["member:app", "member:delivery"]));
        assert_eq!(first["choice"]["option"], "a");
        assert_eq!(wire["records"][1]["evidence"][2]["pin"], "pinned");
    }

    #[test]
    fn supersession_drift_and_trust_survive_the_read() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        let rec = &mut raw["reads"]["records"]["json"]["kinds"][0]["records"][1];
        rec["data"]["supersedes"] = serde_json::json!([{ "decision": "ref-000" }, { "revision": "v1", "option": "b" }]);
        rec["assets"][0]["state"] = "drifted".into();
        rec["attested"] = true.into();
        rec["supersededBy"] = "ref-009".into();
        rec["data"]["choice"] = Value::Null;
        let st = compose(&raw, "local");
        let d = st.records.iter().find(|r| r.id == "ref-002").unwrap();
        assert_eq!(d.supersedes, ["ref-000"]);
        assert_eq!(d.superseded_by.as_deref(), Some("ref-009"));
        assert_eq!(d.evidence.iter().find_map(|e| e.pin), Some(PinState::Drifted));
        assert_eq!(d.attested, Some(true));
        assert_eq!(d.choice, None);
    }

    #[test]
    fn a_gate_carries_its_plan_and_the_member_s_last_release() {
        // #617: status's planDigest, and the member's latest release in the
        // env read (its own op's when it has one).
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        let delivery = &mut raw["reads"]["status"]["json"]["members"][1];
        delivery["gates"][0]["planDigest"] = "sha256:ab12".into();
        let release = |component: &str, at: &str| {
            serde_json::json!({ "component": component, "digest": "d", "gitSha": "1234abcd", "inputDigest": null,
                "runId": "r", "timestamp": at, "actor": "jake", "flags": [], "plan": null })
        };
        delivery["environments"][0]["releases"] =
            serde_json::json!([release("deploy", "2026-10-03T00:00:00Z"), release("ship", "2026-10-01T00:00:00Z")]);
        let st = compose(&raw, "local");
        let why = st.gates[0].why.clone().unwrap();
        assert_eq!(why.plan_digest.as_deref(), Some("sha256:ab12"));
        // release has none of its own: the newest.
        let last = why.last_release.unwrap();
        assert_eq!((last.component.as_str(), last.at.as_str()), ("deploy", "2026-10-03T00:00:00Z"));
        assert_eq!((last.actor.as_deref(), last.git_sha.as_deref()), (Some("jake"), Some("1234abcd")));
        // Not read yet: no decisions, and no note.
        assert_eq!((why.decisions, why.note), (None, None));
        // With its own op's release, that one.
        for g in raw["reads"]["status"]["json"]["members"][1]["gates"].as_array_mut().unwrap() {
            g["state"] = "pending".into();
        }
        let st = compose(&raw, "local");
        let ship = st.gates.iter().find(|g| g.op == "ship").unwrap();
        assert_eq!(ship.why.as_ref().unwrap().last_release.as_ref().unwrap().component, "ship");
        // No releases: none.
        let st = compose(&fixture(include_str!("fixtures/reference-raw.json")), "local");
        assert_eq!(st.gates[0].why.as_ref().unwrap().last_release, None);
    }

    #[test]
    fn the_decisions_covering_a_member_as_chant_ranks_them() {
        // Real `chant workspace graph --intent delivery --json` (0.108.1) on
        // the e2e toy with three decisions: toy-001 constrains
        // member:delivery and supersedes toy-000; toy-002, proposed,
        // constrains one file in it.
        let doc = fixture(include_str!("fixtures/intent-delivery.json"));
        let i = intent(&doc, 1400).unwrap();
        let ids: Vec<(&str, &str, bool)> =
            i.decisions.iter().map(|d| (d.id.as_str(), d.relevance.as_str(), d.current)).collect();
        assert_eq!(ids, [("toy-001", "member", true), ("toy-002", "related", true), ("toy-000", "member", false)]);
        let first = &i.decisions[0];
        assert_eq!(
            (first.title.as_deref(), first.state.as_deref(), first.closed),
            (Some("A person approves each ship"), Some("decided"), false)
        );
        // Only toy-001 governs the member: current, and covering it.
        assert_eq!(i.decisions.iter().filter(|d| d.governs()).map(|d| d.id.as_str()).collect::<Vec<_>>(), ["toy-001"]);
        assert_eq!(i.undecided, 2);
        assert!(i.runs.is_empty());
        assert_eq!(i.ms, 1400);

        // A chant without `why`: the graph's decisions, current first.
        let mut old = doc.clone();
        old.as_object_mut().unwrap().remove("why");
        let i = intent(&old, 0).unwrap();
        assert_eq!(i.decisions.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["toy-001", "toy-002", "toy-000"]);
        assert!(!i.decisions[2].current);
    }

    #[test]
    fn an_intent_read_that_fails_says_why() {
        let failure = serde_json::json!({
            "$schema": "https://intentius.io/chant/schemas/workspace/intent/v1/intent.schema.json",
            "contract": 1, "chant": "0.108.1", "error": { "code": "not-a-git-repository", "message": "not in git" },
        });
        assert_eq!(intent(&failure, 0).unwrap_err(), "not in git (not-a-git-repository)");
        assert!(intent(&Value::Null, 0).is_err());
        let mut other = fixture(include_str!("fixtures/intent-delivery.json"));
        other["contract"] = 2.into();
        assert!(intent(&other, 0).unwrap_err().contains("contract"));
        assert!(INTENT.contains(r#"workspace graph --intent "$3" --json"#));
    }

    #[test]
    fn the_run_ledger_names_each_run_s_pane() {
        // Real `chant workspace runs --json` (0.108.1): one run Arugula
        // started from pane 7, under W-001's lease.
        let runs = runs(&fixture(include_str!("fixtures/runs.json"))).unwrap();
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!((r.id.as_str(), r.pane, r.state.as_deref()), ("arugula-7-1759880000000", Some(7), Some("running")));
        assert_eq!((r.agent.as_deref(), r.unit.as_deref()), (Some("shipper"), Some("W-001")));
        assert_eq!(r.decisions, ["decision/toy-001"]);
        assert_eq!(r.lease.as_deref(), Some("106ffd19-97f5-476c-824e-12ec93af6f28"));
        // Only an Arugula run's id names a pane.
        assert_eq!(RunRef::pane_of("arugula-12-1700000000000"), Some(12));
        for other in ["run-1", "arugula-x-1", "arugula-3", "arugula-3-1-2"] {
            assert_eq!(RunRef::pane_of(other), None, "{other}");
        }
        let failure =
            serde_json::json!({ "contract": 1, "error": { "code": "not-a-git-repository", "message": "not in git" } });
        assert_eq!(super::runs(&failure).unwrap_err(), "not in git");
    }

    #[test]
    fn a_member_s_leases_and_undecided_commits() {
        // A lease in delivery's own ledger is on its card from status alone.
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["reads"]["status"]["json"]["leases"] = serde_json::json!([
            { "item": "W-002", "holder": "sam", "token": "t", "acquiredAt": "a", "expiresAt": "2026-10-08T00:00:00Z",
              "state": "expired", "ref": "refs/chant/lease/_members/delivery/work/W-002", "member": "delivery" },
            { "item": "W-001", "holder": "shipper", "token": "u", "acquiredAt": "a", "expiresAt": "e",
              "state": "active", "ref": "refs/chant/lease/work/W-001", "member": null },
        ]);
        let st = compose(&raw, "local");
        assert_eq!(st.leases.len(), 2);
        let delivery = st.members.iter().find(|m| m.name == "delivery").unwrap();
        assert_eq!(
            delivery.leases.iter().map(|l| (l.item.as_str(), l.state.as_str())).collect::<Vec<_>>(),
            [("W-002", "expired")]
        );
        // The intent read counts the commits no decision covered.
        let i = intent(&fixture(include_str!("fixtures/intent-delivery-work.json")), 0).unwrap();
        assert_eq!(i.work, ["W-001"]);
        let mut m = delivery.clone();
        member_why(&mut m, &st.leases, Some(&Ok(i)), None);
        let w = m.why.unwrap();
        assert_eq!(w.undecided, Some(1));
        assert!(w.runs.is_empty() && w.runs_note.is_none());
        // Both leases now: its own, and W-001's, which covers it.
        assert_eq!(m.leases.iter().map(|l| l.item.as_str()).collect::<Vec<_>>(), ["W-002", "W-001"]);
        // A failed read is a note.
        let mut m = delivery.clone();
        member_why(&mut m, &st.leases, Some(&Err("too slow".into())), Some(&Err("no ledger".into())));
        let w = m.why.unwrap();
        assert_eq!(
            (w.note.as_deref(), w.runs_note.as_deref(), w.decisions),
            (Some("too slow"), Some("no ledger"), None)
        );
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

    /// The fingerprint of a scratch repository, as the block runs it.
    fn print(dir: &std::path::Path) -> String {
        let out = std::process::Command::new("sh").args(["-c", FINGERPRINT, "sh"]).arg(dir).output().unwrap();
        String::from_utf8(out.stdout).unwrap()
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn the_fingerprint_sees_chants_refs_and_takes_no_index_lock() {
        // #305: a lease or a wip snapshot moves the print; status leaves the
        // index alone (no refresh, so no index.lock).
        let dir = std::env::temp_dir().join(format!("arugula-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("chant.workspace.json"), "{}").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "one"]);
        let first = print(&dir);
        assert!(first.starts_with("- "), "{first}");
        assert_eq!(print(&dir), first);

        git(&dir, &["update-ref", "refs/chant/lease/work/fix-001", "HEAD"]);
        let leased = print(&dir);
        assert_ne!(leased, first);
        git(&dir, &["update-ref", "refs/chant/wip/main", "HEAD"]);
        let saved = print(&dir);
        assert_ne!(saved, leased);
        // Neither is the lifecycle word.
        assert_eq!(saved.split_whitespace().next(), Some("-"));

        // A touched file makes the index stale; a refreshing status would
        // rewrite it.
        let index = std::fs::metadata(dir.join(".git/index")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join("chant.workspace.json"), "{}").unwrap();
        print(&dir);
        assert_eq!(std::fs::metadata(dir.join(".git/index")).unwrap().modified().unwrap(), index);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_inside_an_untracked_file_moves_the_fingerprint() {
        // An untracked file was only its `??` line: editing it moved nothing.
        let dir = std::env::temp_dir().join(format!("arugula-fp-untracked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("member")).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("chant.workspace.json"), "{}").unwrap();
        std::fs::write(dir.join(".gitignore"), "ignored/\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "one"]);
        let index = std::fs::metadata(dir.join(".git/index")).unwrap().modified().unwrap();
        std::fs::write(dir.join("member/new file.ts"), "a").unwrap();
        let first = print(&dir);
        // Same `??` line, same size, new contents.
        std::fs::write(dir.join("member/new file.ts"), "b").unwrap();
        let edited = print(&dir);
        assert_ne!(edited, first);
        assert_eq!(print(&dir), edited);
        // Ignored files stay out of it.
        std::fs::create_dir_all(dir.join("ignored")).unwrap();
        std::fs::write(dir.join("ignored/out.js"), "x").unwrap();
        assert_eq!(print(&dir), edited);
        // Still no index write, and the cap the comment states.
        assert_eq!(std::fs::metadata(dir.join(".git/index")).unwrap().modified().unwrap(), index);
        assert!(FINGERPRINT.contains("head -n 1000 "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_quick_look_is_the_fingerprint_s_first_word() {
        let lifecycle = |dir: &std::path::Path| {
            let out = std::process::Command::new("sh").args(["-c", LIFECYCLE, "sh"]).arg(dir).output().unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        let word = |dir: &std::path::Path| print(dir).split_whitespace().next().unwrap().to_owned();
        let dir = std::env::temp_dir().join(format!("arugula-fp-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((lifecycle(&dir), word(&dir)), ("gone".to_owned(), "gone".to_owned()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
        assert_eq!(lifecycle(&dir), "-");
        assert_eq!(lifecycle(&dir), word(&dir));
        git(&dir, &["update-ref", "refs/heads/chant/lifecycle", "HEAD"]);
        assert_ne!(lifecycle(&dir), "-");
        assert_eq!(lifecycle(&dir), word(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_agent_sessions_go_on_their_member() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        raw["agents"] = serde_json::json!([
            { "name": "app", "member": "app" },
            { "name": "design", "member": "design" },
            { "name": "ghost", "member": "nobody" },
        ]);
        let st = compose(&raw, "local");
        let agents = |n: &str| st.members.iter().find(|m| m.name == n).unwrap().agents.clone();
        assert_eq!(agents("app"), ["app"]);
        assert_eq!(agents("design"), ["design"]);
        assert!(agents("delivery").is_empty());
        // A declaration that isn't plain JSON: no sessions, nothing else lost.
        raw["agents"] = Value::Null;
        assert!(compose(&raw, "local").members.iter().all(|m| m.agents.is_empty()));
    }

    #[test]
    fn a_member_s_ops_come_from_status() {
        let mut raw = fixture(include_str!("fixtures/reference-raw.json"));
        // delivery's gates were recorded for release and ship.
        let ops = |st: &State, n: &str| st.members.iter().find(|m| m.name == n).unwrap().ops.clone();
        let st = compose(&raw, "local");
        assert_eq!(ops(&st, "delivery"), ["release", "ship"]);
        assert!(ops(&st, "app").is_empty(), "none named: Run op takes a typed one");
        // A newer chant names its stewards' ops; they come first, once.
        for m in raw["reads"]["status"]["json"]["members"].as_array_mut().unwrap() {
            if m["name"] == "delivery" {
                m["stewards"] = serde_json::json!([{ "name": "s", "ops": [{ "name": "ship" }, { "name": "deploy" }] }]);
            }
        }
        let st = compose(&raw, "local");
        assert_eq!(ops(&st, "delivery"), ["ship", "deploy", "release"]);
    }

    #[test]
    fn the_script_takes_its_arguments_and_no_shell_of_its_own() {
        assert!(SCRIPT.starts_with(r#"cd "$1""#) && SCRIPT.ends_with(r#""$1" "$3""#));
        assert!(!SCRIPT.contains("-ic"));
    }
}
