// The runs page (M215): the exact command a run's S37 operation runs, read
// back from its bytes as Rust writes them (`arugula_e2e::run::Op::encode`),
// a command typed in a box, and how a run is said. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { argvOf, commandLine, httpsOnly, decodeOp, eventDetail, keepLatest, paneLink, runTitle, runsQuery, startedBy, stripAnsi, took, workspaceLabel } from "../src/platform/runs-text.ts";

const b64 = (s) => Buffer.from(s, "utf8").toString("base64");
const op = (lines) => `arugula op v1\ndaemon d1\n${lines.join("\n")}\n`;

test("a command's operation reads back as its arguments", () => {
  const o = decodeOp(op(["kind run", "session -", "pane - -", `cwd ${b64("/srv/app")}`, `argv ${b64("make")}`, `argv ${b64("test it")}`]));
  assert.deepEqual(o, { daemon: "d1", kind: "run", argv: ["make", "test it"], instruction: null, agent: null, directory: "/srv/app", deploy: null });
  assert.equal(commandLine(o.argv), "make 'test it'");
  assert.equal(commandLine(["echo", "it's", ""]), `echo 'it'\\''s' ''`);
});

test("an instruction's operation reads back as its text and agent", () => {
  const o = decodeOp(op(["kind agent", "session -", "pane - -", "cwd -", `text ${b64("Fix the flaky test\nthen push")}`, "agent claude"]));
  assert.equal(o.instruction, "Fix the flaky test\nthen push");
  assert.equal(o.agent, "claude");
  assert.equal(runTitle(op(["kind agent", "session -", "pane - -", "cwd -", `text ${b64("Fix it\nnow")}`, "agent -"])), "Fix it");
});

test("a deploy's operation reads as its deploy, by its environment and commit (UI audit #145)", () => {
  const bytes = op(["kind deploy", "session -", "pane - -", "cwd -", "deploy dep_1", "driver chant-fly", "apply ab12"]);
  const o = decodeOp(bytes);
  assert.deepEqual(o.deploy, { deploy: "dep_1", driver: "chant-fly", apply: "ab12" });
  assert.equal(runTitle(bytes), "Deploy dep_1");
  const deploy = { id: "dep_1", environment: "production", source_kind: "commit", reference: "1a2b3c4d5e6f7a8b" };
  assert.equal(runTitle(bytes, null, deploy), "Deploy production · 1a2b3c4");
  assert.equal(runTitle(bytes, null, { ...deploy, source_kind: "artifact", reference: "sha256:00112233445566778899" }), "Deploy production · 001122334455");
  assert.equal(runTitle(bytes, "nightly", deploy), "nightly", "a spec's name first");
  // A deploy's apply has no command beside it.
  assert.equal(decodeOp(op(["kind deploy", "session -", "pane - -", "cwd -", `argv ${b64("ls")}`, "deploy dep_1", "driver x", "apply y"])), null);
  assert.equal(workspaceLabel({ workspace: "hw_1", workspace_name: "dep_1", deploy }), "its deploy's box");
  assert.equal(workspaceLabel({ workspace: "hw_1", workspace_name: "ci" }), "ci");
  assert.equal(workspaceLabel({ workspace: "hw_1" }), "hw_1");
  assert.equal(workspaceLabel({ workspace: null }), null);
});

test("anything else isn't an operation", () => {
  for (const bad of ["", "arugula op v1\n", "arugula op v2\ndaemon d1\nargv eA==\n", op(["argv !!!"]), op(["kind run", "bogus x"]), "arugula op v1\ndaemon d1\nargv eA=="]) {
    assert.equal(decodeOp(bad), null, JSON.stringify(bad));
  }
  assert.equal(runTitle("nonsense"), "a run");
  assert.equal(runTitle("nonsense", "nightly"), "nightly");
});

test("a typed command splits on spaces, keeping quoted words", () => {
  assert.deepEqual(argvOf(`make test`), ["make", "test"]);
  assert.deepEqual(argvOf(`  sh -c "echo hi && exit 2"  `), ["sh", "-c", "echo hi && exit 2"]);
  assert.deepEqual(argvOf(`echo '' x`), ["echo", "", "x"]);
  assert.equal(argvOf(`echo "open`), null);
  assert.deepEqual(argvOf(""), []);
});

test("how a run is said", () => {
  const names = (a) => ({ e: "Eve" })[a];
  assert.equal(startedBy({ trigger: "manual", started_by: "e", started_by_agent: null, source: {} }, names), "Eve");
  assert.equal(startedBy({ trigger: "manual", started_by: "e", started_by_agent: "m1", source: {} }, names), "Eve's agent");
  assert.equal(startedBy({ trigger: "manual", started_by: "x", started_by_agent: null, source: {} }, names), "someone who left");
  assert.equal(startedBy({ trigger: "schedule", started_by: "rsc_1", started_by_agent: null, source: {} }, names), "its schedule");
  assert.equal(startedBy({ trigger: "event", started_by: "rru_1", started_by_agent: null, source: { event: "pr_opened", repo: "o/r" } }, names), "a rule (pr opened on o/r)");
  assert.equal(took(0, 42_000), "42s");
  assert.equal(took(0, 185_000), "3m 5s");
  assert.equal(took(0, 3_720_000), "1h 2m");
  assert.equal(eventDetail({ reason: "not_hosted: runs go only to hosted workspaces" }), "runs go only to hosted workspaces");
  assert.equal(eventDetail({ exit: 2 }), "exit 2");
  assert.equal(eventDetail({ pane: 3, epoch: 1 }), "in pane %3");
  assert.equal(paneLink("https://control.example", { daemon: "d1", pane: 3 }), "https://control.example/#pane=d1.3");
  assert.equal(paneLink("https://control.example", { daemon: "d1", pane: null }), null);
  assert.equal(runsQuery({ state: "failed", spec: "rsp_1" }), "state=failed&spec=rsp_1&limit=50");
});

test("a log reads as text: its terminal escapes go", () => {
  assert.equal(stripAnsi("\x1b[1;32mok\x1b[0m done\r\n"), "ok done\n");
  assert.equal(stripAnsi("\x1b]0;title\x07a\x1b]8;;https://x\x1b\\link\x1b]8;;\x1b\\b"), "alinkb");
  assert.equal(stripAnsi("\x1b[2K\x1b[1Gprogress\x1b7\x1b8\x1b(B\x1b="), "progress");
  assert.equal(stripAnsi("plain [1m text"), "plain [1m text");
});

test("a followed log keeps its latest pages, about 4 MiB", () => {
  const page = (n, size) => ({ n, text: "x".repeat(size) });
  let pages = [page(0, 3)];
  pages = keepLatest(pages, page(1, 3), 8);
  assert.deepEqual(pages.map((p) => p.n), [0, 1]);
  pages = keepLatest(pages, page(2, 3), 8);
  assert.deepEqual(pages.map((p) => p.n), [1, 2]);
  assert.deepEqual(keepLatest(pages, page(3, 20), 8).map((p) => p.n), [3], "never the newest");
  assert.equal(keepLatest([], page(0, 1)).length, 1);
});

test("a chant ledger link is a link only when it's https (M220)", () => {
  assert.equal(httpsOnly("https://github.com/acme/app/blob/" + "a".repeat(40) + "/github/releases.jsonl#L3"), true);
  for (const bad of ["javascript:alert(1)", "http://github.com/x", "data:text/html,x", "chant:acme/app", "not a url"]) assert.equal(httpsOnly(bad), false, bad);
});
