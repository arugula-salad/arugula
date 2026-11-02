// The chant→Fly driver, on a deploy's box (M220, platform step 7, native
// Arugula #16; aru-104, aru-126). arugulad runs it as the run user with
// `node`, its secrets already in its environment (read from its standard
// input by the driver's shell, never a file or a command line), its output
// redacted into the run's pane, and fd 3 a pipe for its result
// (`work_deploy.rs`).
//
// What it does is what the deploy's plan says
// (`arugula_e2e::drivers::plan_text`), in one of two phases
// (DEPLOY_PHASE, #1985):
//
// - Its plan (`plan`), on a box of its own before anyone approves it, with
//   GITHUB_TOKEN only (minted for the run) and no Fly token: check out the
//   commit, exactly; in the chant member, `npm ci` (with `--ignore-scripts`
//   unless the `npm_scripts` setting says otherwise), then `chant run <op>
//   --json`, which stops at the op's gate (only the `chant_gates`
//   setting's, when it names some) and records its pending fact with
//   chant's plan digest; push chant's ledger to origin. Its result is that
//   gate (`chant_gate`), which the owner's passkey signs chant's statement
//   of. An op that needs Fly before its gate fails here.
// - Its apply (`apply`), on another box once an owner approved it: check
//   FLY_API_TOKEN is a Fly deploy token for one app that expires within 30
//   days (`flyctl tokens debug`); check out the commit; `npm ci`; `chant
//   plan stale` of the approved plan, refused when anything applied to its
//   targets since; `chant approve <op> <gate> --statement` as the approver
//   (`arugula:<account>`, relayed by arugula, with the approval's binding
//   line), checked to be the statement the passkey signed; `chant approve
//   --at --signature -` with the passkey's assertion, which chant checks
//   against `.chant/trust.json`; then `chant run <op>` again, which applies.
//   Then find chant's new release record for this commit in the `chant_env`
//   ledger environment, tie it to what Fly runs, and push chant's ledger.
// - Roll back (an apply with DEPLOY_ROLL_BACK): `flyctl deploy --image` of
//   the image the deploy it goes back to ran (Fly's own roll back), check
//   the app runs it, then `chant components release` records it in
//   chant's ledger again. No gate, so no plan first.
// - Check the Fly app: every started machine's checks passing, on one
//   image.
//
// chant 0.124.64 or newer: the first that signs and checks a binding line
// (chant#4697). An older chant is refused.
//
// Who gets which credential:
// - the repo's chant op: FLY_API_TOKEN (in an apply: it deploys), and
//   GITHUB_TOKEN only with the `chant_github` setting, in an apply;
// - flyctl: FLY_API_TOKEN;
// - the driver's own git fetch, push and ls-remote: GITHUB_TOKEN (a GitHub
//   App installation token the platform mints for this run, writing this
//   repository's contents only, or in an apply the environment's own),
//   through a credential helper for https://github.com that names the
//   variable;
// - `npm ci`, `chant approve`, `chant plan stale` and `chant components
//   release`: neither.
// Neither token is ever on a command line or in a file. Without
// GITHUB_TOKEN, chant's own git reads and writes its ledger through a
// stand-in remote here, and the driver pushes it.
//
// Its result goes on fd 3, which only it holds (every program it runs starts
// with fd 3 closed): chant's gate (a plan), or chant's ledger entry,
// chant's record and Fly's image and health (an apply), which arugulad
// reports with the run's end, and the platform checks and keeps.

import { spawnSync } from "node:child_process";
import { closeSync, existsSync, fstatSync, writeFileSync, writeSync } from "node:fs";
import { join } from "node:path";

// The result's pipe, written as fd 3 itself. Reopening it (/dev/fd/3) would
// check the pipe's mode, which the daemon's user made and the run user can't
// open; node can't mark it close-on-exec, so every program this runs starts
// through `noFd3`, which closes it first.
let resultFd = null;
try {
  if (fstatSync(3).isFIFO()) resultFd = 3;
} catch {
  resultFd = null;
}
/** `argv`, started with fd 3 (the result pipe) closed. */
const noFd3 = (argv) => ["/bin/sh", "-c", 'exec 3>&- "$@"', "sh", ...argv];

const env = process.env;
const say = (s) => process.stdout.write(`chant-fly: ${s}\n`);
const fail = (s, code = 1) => {
  process.stderr.write(`chant-fly: ${s}\n`);
  process.exit(code);
};
if (resultFd === null) fail("there's no pipe for its result (fd 3)");

// Knobs for its tests only (a local origin, quick Fly checks): never on a
// box, whose daemon sets nothing named ARUGULA_*.
const testing = env.ARUGULA_CHANT_FLY_TEST === "1";
const settings = JSON.parse(env.DEPLOY_SETTINGS || "{}");
const rollBack = env.DEPLOY_ROLL_BACK ? JSON.parse(env.DEPLOY_ROLL_BACK) : null;
const app = settings.fly_app;
const chantEnv = settings.chant_env;
const chantDir = settings.chant_dir || ".";
const op = settings.chant_op || "release";
const gatesAllowed = settings.chant_gates ? settings.chant_gates.split(",") : null;
const sha = env.DEPLOY_SOURCE_REF;
const where = env.DEPLOY_SOURCE_WHERE;
// Its plan, before anyone approves it, or its apply (#1985).
const planning = env.DEPLOY_PHASE === "plan";
if (!planning && env.DEPLOY_PHASE !== "apply") fail("DEPLOY_PHASE is plan or apply");
const approver = planning ? null : env.DEPLOY_APPROVER;
if (!app || !chantEnv || !where || (!planning && !approver)) fail("the deploy's settings, source or approver are missing");
if (env.DEPLOY_SOURCE_KIND !== "commit" || !/^[0-9a-f]{40}$/.test(sha || "")) fail("chant-fly deploys a commit, by its full id");
if (planning && rollBack) fail("a roll back goes through no gate, so it has no plan to run");
if (planning && env.FLY_API_TOKEN) fail("a deploy's plan runs without FLY_API_TOKEN, and this one was given it: nothing runs");
if (planning && !env.GITHUB_TOKEN) fail("a deploy's plan needs GITHUB_TOKEN to push chant's ledger, and has none");
if (!planning && !env.FLY_API_TOKEN) fail("the environment gave no FLY_API_TOKEN: set it (fly tokens create deploy -a <app> -x 720h)");
// What the owner's passkey signed (v3): chant's gate, the statement's time
// and binding, the approver's principal, its challenge and the assertion.
// An apply that isn't a roll back has one, or nothing is approved.
let relay = null;
if (!planning && !rollBack) {
  try {
    relay = JSON.parse(env.DEPLOY_CHANT || "null");
  } catch {
    relay = null;
  }
  const g = relay?.gate;
  const ok = (v) => typeof v === "string" && v.length > 0;
  if (!g || ![g.op, g.gate, g.plan, relay.at, relay.binding, relay.principal, relay.relay, relay.challenge].every(ok) || !relay.signature)
    fail("the approval isn't one over chant's gate statement (approval v3): approve this deploy again");
  if (g.op !== op) fail(`the approval is for op ${g.op}'s gate, and this environment's op is ${op}: not approved`);
  if (relay.principal !== `arugula:${approver}`) fail(`the approval's principal ${relay.principal} isn't the approver's`);
}

// Each program's environment: the run's, with only the credentials it's
// given. chant says arugula ran it; who approved is the approver.
const { FLY_API_TOKEN, GITHUB_TOKEN, ARUGULA_CHANT_FLY_TEST, DEPLOY_CHANT, ...rest } = env;
const base = { ...rest, USER: "arugula", LOGNAME: "arugula" };
const withFly = { ...base, FLY_API_TOKEN };
const withGit = GITHUB_TOKEN ? { ...base, GITHUB_TOKEN } : base;
// A plan's op gets no GitHub token whatever the settings say: nobody has
// approved anything yet.
const chantGithub = !planning && settings.chant_github === "true" && !!GITHUB_TOKEN;

/** Run `argv` in `cwd` with `penv`: its output shown, `{status, stdout}` back. `input` goes on its standard input. */
function run(argv, { cwd, penv = base, capture = false, check = true, input } = {}) {
  say(`$ ${argv.join(" ")}`);
  const [cmd, ...args] = noFd3(argv);
  const r = spawnSync(cmd, args, {
    cwd,
    env: penv,
    stdio: [input === undefined ? "ignore" : "pipe", capture ? "pipe" : "inherit", "inherit"],
    input,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (r.error) fail(`${argv[0]}: ${r.error.message}`);
  if (check && r.status !== 0) fail(`${argv.slice(0, 3).join(" ")} exited ${r.status}`);
  return { status: r.status, stdout: r.stdout ?? "" };
}
const src = join(env.HOME, "src");
const git = (args, opts = {}) => run(["git", ...args], { cwd: src, ...opts });
const out = (args, opts = {}) => git(args, { capture: true, ...opts }).stdout.trim();
const remoteTip = () => out(["ls-remote", "origin", "refs/heads/chant/lifecycle"], { penv: withGit }).split(/\s/)[0];

// The source, at the approved commit.
const gitBase = testing && env.DEPLOY_GIT_BASE ? env.DEPLOY_GIT_BASE : "https://github.com/";
// The helper answers only with a token: an empty password is refused even
// where no token is needed (a public repository).
const helper = `!f() { test "$1" = get && test -n "$GITHUB_TOKEN" && printf 'username=x-access-token\\npassword=%s\\n' "$GITHUB_TOKEN"; }; f`;
const originUrl = `${gitBase}${where}.git`;
run(["git", "init", "-q", src]);
git(["remote", "add", "origin", originUrl]);
git(["config", "credential.https://github.com.helper", helper]);
git(["config", "user.name", `arugula (deploy ${env.DEPLOY_ID})`]);
git(["config", "user.email", "deploy@arugula.invalid"]);
git(["fetch", "-q", "origin", "+refs/heads/*:refs/remotes/origin/*", "--tags"], { penv: withGit });
git(["checkout", "-q", "--detach", sha]);
const at = out(["rev-parse", "HEAD"]);
if (at !== sha) fail(`checked out ${at}, not ${sha}`);
// chant's ledger as origin has it, before this run: what's new is this
// run's (an apply's starts with its plan's pending fact).
const before = out(["rev-parse", "-q", "--verify", "refs/remotes/origin/chant/lifecycle"], { check: false });
if (before) git(["branch", "-q", "-f", "chant/lifecycle", before]);
// chant's ledger goes to origin: without a push, its entry would go with
// this box. Checked before anything changes.
if (git(["push", "--dry-run", "-q", "origin", "HEAD:refs/heads/chant/arugula-push-check"], { penv: withGit, check: false }).status !== 0)
  fail(
    `origin won't take a push, so chant's ledger can't reach it: the run's GITHUB_TOKEN (minted through the platform's ` +
      `GitHub App, which must be installed on ${where}, or the environment's own) must be able to push`,
  );

// chant reads its ledger from its remote before it writes to it, and
// refuses when it can't (a first deploy, with no chant/lifecycle here yet,
// would read as unknown, not empty). Without a token of its own it can't
// reach GitHub, so its ledger goes to a stand-in instead: a repository here
// holding chant/lifecycle as origin had it (or none, as origin has none),
// which takes that branch and nothing else. The driver pushes what chant
// added to origin itself. How chant is pointed at it depends on chant's
// version (LEDGER_REMOTE, below).
let standIn = null;
if (!chantGithub) {
  standIn = join(env.HOME, "ledger.git");
  run(["git", "init", "-q", "--bare", standIn]);
  writeFileSync(
    join(standIn, "hooks", "update"),
    '#!/bin/sh\n[ "$1" = refs/heads/chant/lifecycle ] || { echo "only chant/lifecycle goes here" >&2; exit 1; }\n',
    { mode: 0o755 },
  );
  if (before) git(["push", "-q", standIn, `${before}:refs/heads/chant/lifecycle`]);
}

// The token: a deploy token for one app that expires within 30 days, as
// far as flyctl can tell (its caveats).
// (A plan runs no flyctl: it has no Fly token.)
const flyctl = planning
  ? null
  : spawnSync("/bin/sh", noFd3(["flyctl", "version"]).slice(1), { env: base }).status === 0
    ? "flyctl"
    : "fly";
function tokenOk() {
  const r = run([flyctl, "tokens", "debug"], { penv: withFly, capture: true, check: false });
  if (r.status !== 0) return "flyctl tokens debug didn't read it";
  let macs;
  try {
    macs = JSON.parse(r.stdout);
  } catch {
    return "flyctl tokens debug's output didn't read";
  }
  return tokenScope((Array.isArray(macs) ? macs : []).flatMap((m) => (Array.isArray(m?.caveats) ? m.caveats : [])));
}

/** Why a token with `caveats` (each of its macaroons', top level) won't do: `null` when it will. */
function tokenScope(caveats) {
  // Its app: an Apps caveat for one app, at the top level (every access
  // must name that app), or in an IfPresent caveat (`fly tokens create
  // deploy`'s: Apps and FeatureSet, with `else` for every access that names
  // neither). An IfPresent caveat counts only when its `else` reads at most
  // and it holds nothing that reaches past the app but Fly's features
  // (and windows, which only narrow): IfPresent{ifs: [Apps{one}], else:
  // "rwcdC"} could still change the whole org.
  const apps = [];
  for (const c of caveats) {
    if (c?.type === "Apps") apps.push(c);
    if (c?.type !== "IfPresent") continue;
    const ifs = Array.isArray(c?.body?.ifs) ? c.body.ifs : [];
    if (!ifs.some((i) => i?.type === "Apps")) continue;
    if (!readsAtMost(c?.body?.else))
      return "it isn't scoped to one app (its IfPresent caveat lets more than read past the app)";
    if (ifs.some((i) => !["Apps", "FeatureSet", "ValidityWindow"].includes(i?.type)))
      return `it isn't scoped to one app (its IfPresent caveat holds ${ifs.map((i) => i?.type).join(", ")})`;
    apps.push(...ifs.filter((i) => i?.type === "Apps"));
  }
  const counts = apps.map((c) => Object.keys(c?.body?.apps ?? {}).length);
  if (counts.length === 0 || counts.some((n) => n !== 1)) return "it isn't scoped to one app";
  // When it ends: its top-level windows only (one in an IfPresent caveat
  // binds only the accesses it's relevant to).
  const ends = caveats.filter((c) => c?.type === "ValidityWindow").map((c) => Number(c?.body?.not_after));
  const now = Date.now() / 1000;
  const end = Math.min(...ends.filter(Number.isFinite));
  if (!Number.isFinite(end)) return "it doesn't expire";
  if (end <= now) return "it expired";
  if (end - now > (720 + 1) * 3600) return "it lives longer than 30 days";
  return null;
}

/** Whether a macaroon action (`"r"`, `"rwcdC"`, `"*"`: superfly/macaroon's resset.Action) is read or nothing. */
function readsAtMost(action) {
  return typeof action === "string" && /^r?$/.test(action);
}
if (!planning) {
  const bad = tokenOk();
  if (bad) fail(`FLY_API_TOKEN won't do: ${bad}. Make one with: fly tokens create deploy -a ${app} -x 720h`);
}

// chant, as the member pins it, its packages fetched without the tokens.
const member = join(src, chantDir);
if (!existsSync(member)) fail(`there's no ${chantDir} in ${where} at ${sha}`);
if (existsSync(join(member, "package-lock.json"))) {
  const scripts = settings.npm_scripts === "true" ? [] : ["--ignore-scripts"];
  run(["npm", "ci", "--no-fund", "--no-audit", ...scripts], { cwd: member, penv: base });
}
const pinned = join(member, "node_modules", ".bin", "chant");
const chant = (args, opts = {}) => run([existsSync(pinned) ? pinned : "chant", ...args], { cwd: member, ...opts });

/** Whether chant `version` (`x.y.z`) is `floor` or newer. */
function atLeast(version, floor) {
  const a = version.split(".").map(Number);
  const b = floor.split(".").map(Number);
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return true;
}

// chant 0.124.64 or newer: the first that signs and checks a binding line
// (chant#4697), which an approval over chant's statement carries. An older
// one is refused, whatever the phase.
const CHANT_FLOOR = "0.124.64";
const said = chant(["--version"], { capture: true, check: false });
const chantVersion = said.status === 0 ? /^\d+\.\d+\.\d+/.exec(said.stdout.trim())?.[0] : undefined;
if (!chantVersion || !atLeast(chantVersion, CHANT_FLOOR))
  fail(
    `${chantDir} pins chant ${chantVersion ?? "(its version didn't read)"}, and chant-fly needs ${CHANT_FLOOR} or newer, ` +
      `which checks the passkey's signature and its binding line: update @intentius/chant in ${chantDir}'s package.json`,
  );

// The stand-in as chant's ledger remote, by name (CHANT_LIFECYCLE_REMOTE,
// chant#4647), so origin stays GitHub for everything else chant reads.
const LEDGER_REMOTE = "arugula-ledger";
const chantGit = {};
if (standIn) {
  git(["remote", "add", LEDGER_REMOTE, standIn]);
  chantGit.CHANT_LIFECYCLE_REMOTE = LEDGER_REMOTE;
}
// chant's own commands get no token; its op gets the Fly token in an
// apply, and GitHub's with `chant_github`. A plan's op gets neither.
const chantBase = { ...base, ...chantGit };
const forChant = planning ? chantBase : chantGithub ? { ...chantBase, FLY_API_TOKEN, GITHUB_TOKEN } : { ...chantBase, FLY_API_TOKEN };

// Fly: every started machine's checks passing, all on one image.
function flyHealth() {
  const r = run([flyctl, "status", "--app", app, "--json"], { penv: withFly, capture: true, check: false });
  if (r.status !== 0) return { healthy: false, why: `flyctl status exited ${r.status}` };
  let st;
  try {
    st = JSON.parse(r.stdout);
  } catch {
    return { healthy: false, why: "flyctl status's --json didn't read" };
  }
  const started = (st.Machines || []).filter((m) => m.state === "started");
  const images = [
    ...new Set(
      started.map((m) => {
        const i = m.image_ref || {};
        return `${i.registry}/${i.repository}${i.tag ? `:${i.tag}` : ""}@${i.digest}`;
      }),
    ),
  ];
  const failing = started.filter((m) => (m.checks || []).some((c) => c.status !== "passing"));
  if (started.length === 0) return { healthy: false, why: "no machine is started" };
  if (failing.length > 0) return { healthy: false, why: `machine ${failing[0].id}'s checks aren't passing`, image: images[0] };
  if (images.length !== 1) return { healthy: false, why: `its machines run ${images.length} images` };
  return { healthy: true, image: images[0] };
}

const note = planning
  ? null
  : `arugula deploy ${env.DEPLOY_ID}: approval ${env.DEPLOY_APPROVAL || "(no id)"} by ${approver}, ` +
    `operation sha256:${env.DEPLOY_OP_HASH}, run ${env.DEPLOY_RUN}`;
let beforeImage = null;

/** The pending fact chant's run `id` wrote for gate `gate` of the op, in the ledger here, with a plan digest. */
function pendingFact(gate, id) {
  const facts = out(["show", `refs/heads/chant/lifecycle:_gates/${op}.jsonl`], { check: false }).split("\n");
  for (const line of facts.reverse()) {
    let f;
    try {
      f = JSON.parse(line);
    } catch {
      continue;
    }
    const mine = f?.kind === "pending" && f.op === op && f.gate === gate && f.runId === id;
    if (mine && typeof f.planDigest === "string" && /^(jcs1-)?sha256:[0-9a-f]{64}$/.test(f.planDigest)) return f;
  }
  return null;
}

/** `chant run <op> --json`'s record, from a run that stopped at a gate: its gate's name, checked. */
function gatedAt(r) {
  // `chant run <op> --json` prints the run's record: its `op`, its `id`,
  // `gate: {name, since}` and the command that approves it.
  let rec;
  try {
    rec = JSON.parse(r.stdout);
  } catch {
    rec = null;
  }
  const gate = rec?.gate?.name;
  if (typeof gate !== "string" || !gate) fail(`chant run ${op} stopped at a gate, and its --json didn't say which`);
  if (rec.op !== op) fail(`chant run ${op} stopped at another op's gate (${rec.op} ${gate}): not approved`);
  if (gatesAllowed && !gatesAllowed.includes(gate))
    fail(`chant run ${op} stopped at gate ${gate}, which the plan doesn't name: not approved`);
  return { rec, gate };
}

let chantGate = null;
if (planning) {
  // The plan (#1985): chant's op, with no Fly token, to its gate. chant
  // records the pending fact and its plan digest on its ledger; that gate
  // is what the owner approves, over chant's own statement of it.
  const r = chant(["run", op, "--json"], { penv: forChant, capture: true, check: false });
  if (r.status === 0)
    fail(`chant run ${op} finished without stopping at a gate, so there's nothing for a person to approve: give the op a gate`);
  if (r.status !== 3)
    fail(`chant run ${op} exited ${r.status} before its gate (an op that needs FLY_API_TOKEN before its gate can't plan here)`, r.status || 1);
  const { rec, gate } = gatedAt(r);
  const fact = pendingFact(gate, rec.id);
  if (!fact) fail(`chant run ${op} stopped at gate ${gate}, and chant's ledger has no plan digest for it (run ${rec.id})`);
  const hinted = /--plan (\S+)/.exec(typeof rec.approve === "string" ? rec.approve : "")?.[1];
  if (hinted && hinted !== fact.planDigest)
    fail(`chant run ${op} says to approve plan ${hinted}, its ledger ${fact.planDigest}: not planned`);
  chantGate = {
    chant: chantVersion,
    op,
    gate,
    ...(typeof fact.environment === "string" ? { environment: fact.environment } : {}),
    plan: fact.planDigest,
    ...(typeof fact.checkpointDigest === "string" ? { checkpoint: fact.checkpointDigest } : {}),
    run: rec.id,
    recorded: fact.timestamp,
  };
  say(`chant run ${op} stopped at gate ${gate} with plan ${fact.planDigest}: an owner approves that plan`);
} else if (!rollBack) {
  beforeImage = flyHealth().image ?? null;
  const g = relay.gate;
  // chant says whether anything applied to the plan's targets since it
  // was recorded (chant#4648): a laptop's apply, another CI's, ours.
  const stale = chant(["plan", "stale", g.plan, "--json"], { penv: chantBase, capture: true, check: false });
  if (stale.status === 2) {
    let n = "";
    try {
      n = ` (${JSON.parse(stale.stdout).applies.length} since)`;
    } catch {
      n = "";
    }
    fail(`chant says plan ${g.plan} is stale: something applied to its targets after it was recorded${n}. Plan the deploy again`);
  }
  if (stale.status !== 0) fail(`chant plan stale ${g.plan} exited ${stale.status}: the plan's freshness couldn't be read`);
  // The approval, as chant records it: the statement chant makes for it
  // must be the one the passkey signed, and chant checks the signature
  // against the passkeys .chant/trust.json lists for the approver.
  const args = [
    "approve", op, g.gate,
    "--actor", relay.principal,
    "--relayed-by", relay.relay,
    "--plan", g.plan,
    ...(g.environment ? ["--env", g.environment] : []),
    "--binding", relay.binding,
    "--at", relay.at,
  ];
  const st = chant([...args, "--statement"], { penv: chantBase, capture: true, check: false });
  let statement = null;
  try {
    statement = JSON.parse(st.stdout);
  } catch {
    statement = null;
  }
  if (st.status !== 0 || !statement) fail(`chant approve ${op} ${g.gate} --statement exited ${st.status}: not approved`);
  if (statement.challenge !== relay.challenge)
    fail(
      `chant's statement for this approval isn't the one the passkey signed (chant's challenge ${statement.challenge}, ` +
        `the passkey's ${relay.challenge}): not approved`,
    );
  say(`chant run ${op} waits at gate ${g.gate}: approving plan ${g.plan} as ${relay.principal}, signed with a passkey`);
  chant([...args, "--note", note, "--signature", "-"], { penv: chantBase, input: JSON.stringify(relay.signature) });
  const r = chant(["run", op, "--json"], { penv: forChant, capture: true, check: false });
  if (r.status === 3) {
    const { gate } = gatedAt(r);
    fail(
      gate === g.gate
        ? `chant run ${op} stopped at gate ${gate} again: chant's plan now isn't the one approved. Plan the deploy again`
        : `chant run ${op} stopped at gate ${gate}, which this approval doesn't cover: approve it as a deploy of its own`,
    );
  }
  if (r.status !== 0) fail(`chant run ${op} exited ${r.status}`, r.status || 1);
} else {
  // Fly's own roll back: the image the deploy it goes back to ran.
  const args = [flyctl, "deploy", "--app", app, "--image", rollBack.to.image, "--yes"];
  if (settings.fly_config) args.push("--config", join(src, settings.fly_config));
  run(args, { cwd: src, penv: withFly });
}

if (planning) {
  // The pending fact goes to origin, where the apply's box reads it.
  const tip = out(["rev-parse", "-q", "--verify", "refs/heads/chant/lifecycle"], { check: false });
  if (tip && tip !== before && remoteTip() !== tip)
    git(["push", "-q", "origin", "refs/heads/chant/lifecycle:refs/heads/chant/lifecycle"], { penv: withGit, check: false });
  const pushed = !!tip && remoteTip() === tip;
  writeSync(resultFd, JSON.stringify({ chant_gate: chantGate }));
  closeSync(resultFd);
  if (!pushed) fail("chant's ledger, with its plan, didn't reach origin", 4);
  say(`planned ${sha} for ${app}: chant's gate ${chantGate.gate}, plan ${chantGate.plan}`);
  process.exit(0);
}

const [tries, every] = (testing && env.DEPLOY_FLY_CHECKS ? env.DEPLOY_FLY_CHECKS : "12x10").split("x").map(Number);
let fly;
for (let i = 0; i < tries; i++) {
  fly = flyHealth();
  if (fly.healthy && (!rollBack || fly.image === rollBack.to.image)) break;
  if (fly.healthy) fly = { healthy: false, why: `it runs ${fly.image}, not ${rollBack.to.image}`, image: fly.image };
  say(`Fly app ${app} isn't healthy yet: ${fly.why}`);
  if (i + 1 < tries) spawnSync("/bin/sh", noFd3(["sleep", String(every)]).slice(1));
}

if (rollBack) {
  // chant's ledger says it runs again, as the approver: chant's own
  // command, without the tokens.
  chant(
    [
      "components", "release", rollBack.to.chant_env,
      "--component", rollBack.to.chant_component,
      "--digest", rollBack.to.chant_digest,
      "--git-sha", sha,
      "--run-id", env.DEPLOY_RUN,
      "--actor", "arugula",
      "--approver", `arugula:${approver}`,
    ],
    { penv: chantBase },
  );
}

// chant's ledger entry: this run's newest release record for this commit,
// in the plan's ledger environment, pushed to origin.
let ledger = null;
let record = null;
let why = null;
const tip = out(["rev-parse", "-q", "--verify", "refs/heads/chant/lifecycle"], { check: false });
if (tip && tip !== before) {
  if (remoteTip() !== tip) git(["push", "-q", "origin", "refs/heads/chant/lifecycle:refs/heads/chant/lifecycle"], { penv: withGit, check: false });
  const pushed = remoteTip() === tip;
  const files = out(["ls-tree", "-r", "--name-only", tip])
    .split("\n")
    .filter((f) => f === `${chantEnv}/releases.jsonl` || f.endsWith(`/${chantEnv}/releases.jsonl`));
  for (const f of files) {
    const had = before ? out(["show", `${before}:${f}`], { check: false }).split("\n").filter(Boolean).length : 0;
    out(["show", `${tip}:${f}`]).split("\n").forEach((line, n) => {
      if (n < had) return; // there before this run
      let r;
      try {
        r = JSON.parse(line);
      } catch {
        return;
      }
      if (r.gitSha === sha && r.env === chantEnv && (!record || String(r.timestamp) >= String(record.timestamp))) {
        record = r;
        ledger = { path: f, line: n + 1 };
      }
    });
  }
  if (ledger && !pushed) {
    why = "chant's ledger didn't reach origin";
    ledger = null;
  }
}
if (!ledger && !why) why = `chant's ledger has no new release record for ${sha} in ${chantEnv}`;
if (ledger) ledger = `https://github.com/${where}/blob/${tip}/${ledger.path}#L${ledger.line}`;

// The record is this release's: its digest is the image's (`chant_digest
// image`), or Fly runs another image than before (`plan`).
let tied = true;
if (record && !rollBack && fly?.image) {
  if (settings.chant_digest === "image") tied = record.digest === `sha256:${fly.image.split("@sha256:")[1]}`;
  else tied = fly.image !== beforeImage;
}

const result = {
  chant_ledger: ledger,
  chant_record: record && {
    component: record.component,
    env: record.env,
    digest: record.digest,
    gitSha: record.gitSha,
    runId: record.runId,
    timestamp: record.timestamp,
  },
  fly: { app, image: fly?.image ?? null, healthy: !!fly?.healthy, ...(fly?.why ? { why: fly.why } : {}) },
};
writeSync(resultFd, JSON.stringify(result));
closeSync(resultFd);
if (!fly?.healthy) fail(`Fly app ${app} isn't healthy: ${fly?.why}`, 5);
if (!ledger) fail(why, 4);
if (!tied)
  fail(
    settings.chant_digest === "image"
      ? `chant's release record names ${record.digest}, not the image Fly runs`
      : `Fly runs the image it ran before the release (${beforeImage}): the release didn't reach it`,
    6,
  );
say(rollBack ? `rolled ${app} back to ${rollBack.to.image}` : `released ${sha} to ${app}`);
