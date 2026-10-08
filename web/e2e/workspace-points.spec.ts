// #621's "done when", against the real chant (0.108.1, the one
// e2e/fixtures/chant-workspace-decisions pins): an open decision point is a
// card on the swarm's rail that can be answered from the phone, and the
// answer is a points record chant reads.
//
// The toy workspace gets an answer kind (`answers/`, with the schema copy
// chant ships), three points (`decisions/points.json`) and an op whose gate
// asks one of them (chant#3170). A question asked with `points ask` is
// escalated to people; the gate's question is asked by `chant run ask`.
//
// The first run installs the fixture's chant (`npm ci`, a few seconds).

import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { open, closeContexts } from "./helpers";
import type { PaneId, Reason } from "../src/proto";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FIXTURE = fileURLToPath(new URL("./fixtures/chant-workspace-decisions", import.meta.url));
const phone = (() => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  return { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch };
})();

let PORT = 0;
let dir = "";
let ws = "";
let daemon: ChildProcess | null = null;
let block: PaneId = 0;

const base = () => `http://127.0.0.1:${PORT}`;
const chant = (cwd: string, ...args: string[]) =>
  spawnSync(join(FIXTURE, "node_modules/.bin/chant"), args, { cwd, encoding: "utf8", env: { ...process.env, NO_COLOR: "1" } });
const git = (...args: string[]) => execFileSync("git", ["-C", ws, "-c", "user.email=t@example.com", "-c", "user.name=t", ...args]);
const cli = (...args: string[]) => execFileSync("../target/debug/arugula", ["--socket", join(dir, "state/sock"), ...args], { encoding: "utf8" });
const panesOf = (page: Page) => page.evaluate(() => window.__arugula?.client.state?.panes ?? []);
const reasonOf = async (page: Page, id: PaneId): Promise<Reason | null> => (await panesOf(page)).find((p) => p.id === id)?.reason ?? null;

interface Question {
  id: string;
  point: string;
  state: string;
  answer: string | boolean | null;
  answeredBy: string[];
}
/** Every question chant has, by its own read. */
const questions = (): Question[] => JSON.parse(chant(ws, "workspace", "points", "--json").stdout).questions;
const question = (point: string) => questions().find((q) => q.point === point);

const POINTS = {
  $schema: "https://intentius.io/chant/schemas/workspace/decision-points/v1/decision-points.schema.json",
  points: {
    "slice-tier": {
      title: "Which builder tier builds this work item",
      question: {
        type: "choice",
        instructions: "Pick the smallest builder tier that can build this work item.",
        criteria: { small: "A small builder.", medium: "A mid-size builder.", large: "The largest builder." },
      },
      inputs: { "work-item.words": "words in the body" },
      deciders: [{ kind: "quorum", count: 1 }],
    },
    "ship-ok": {
      title: "Ship the toy now",
      question: { type: "noul", instructions: "Should the toy ship?", criteria: { true: "Ship it.", false: "Hold it." } },
      inputs: { "gate.component": "the Op" },
      deciders: [{ kind: "quorum", count: 1 }],
    },
  },
};

const ANSWER_KIND = `export const recordKind = {
  name: "answer",
  location: { dir: ".", match: "^[a-z][a-z0-9-]*-[0-9a-f]{12}\\\\.md$" },
  format: "markdown-front-matter",
  schema: { id: "urn:intentius:chant:point-answer:1", path: "answer.schema.json" },
  idField: "id",
  stateField: "state",
  states: ["escalated", "proposed", "answered"],
  closedStates: ["answered"],
  constrains: { field: "constrains" },
  source: { field: "source" },
  answers: { points: "../decisions/points.json" },
};
`;

const ASK_OP = `import { Op, phase, gate, shell } from "@intentius/chant/op";

export default Op({
  name: "ask",
  overview: "Ask whether to ship, then ship",
  phases: [phase("Ask", [gate("ship-ok", { point: "ship-ok", description: "Ship the toy" })]), phase("Ship", [shell("echo shipped")])],
});
`;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(base()) });

test.beforeAll(async () => {
  test.setTimeout(180_000);
  if (!existsSync(join(FIXTURE, "node_modules/.bin/chant"))) {
    execFileSync("npm", ["ci", "--no-audit", "--no-fund"], { cwd: FIXTURE, stdio: "ignore" });
  }
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-e2e-workspace-points-")));
  ws = join(dir, "toy");
  cpSync(FIXTURE, ws, { recursive: true, filter: (src) => !src.includes("node_modules") });
  symlinkSync(join(FIXTURE, "node_modules"), join(ws, "node_modules"));
  // The answer kind, its points, and an op whose gate asks one.
  mkdirSync(join(ws, "answers"));
  writeFileSync(join(ws, "answers", "answer.kind.mjs"), ANSWER_KIND);
  copyFileSync(join(FIXTURE, "node_modules/@intentius/chant/dist/workspace/point-answer.schema.json"), join(ws, "answers", "answer.schema.json"));
  writeFileSync(join(ws, "decisions", "points.json"), JSON.stringify(POINTS, null, 2));
  writeFileSync(join(ws, "delivery", "ask.op.ts"), ASK_OP);
  const decl = JSON.parse(readFileSync(join(ws, "chant.workspace.json"), "utf8"));
  decl.records.push({ kind: "answers/answer.kind.mjs" });
  writeFileSync(join(ws, "chant.workspace.json"), JSON.stringify(decl, null, 2));
  git("init", "-q", "-b", "main");
  git("add", ".");
  git("commit", "-qm", "the toy workspace, with decision points");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(join(dir, "state")), "--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--wisp-token-file", "/nonexistent"],
    ],
    { stdio: "ignore" },
  );
  PORT = await daemonPort(join(dir, "state"), daemon);
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${base()}/api/host`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("daemon did not start");
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

test("an open decision point is a card on the rail, answered from the phone, and chant reads the answer (#621)", async ({ browser, page }, info) => {
  test.setTimeout(120_000);
  const asked = chant(ws, "workspace", "points", "ask", "slice-tier", "--inputs", '{"work-item.words": 300}', "--subject", "W-001");
  expect(asked.status, asked.stdout + asked.stderr).toBe(0);
  expect(question("slice-tier")?.state).toBe("escalated");
  const out = cli("workspace", ws);
  block = Number(/^%(\d+)/.exec(out)![1]);
  expect(out).toContain("0 waiting at a gate, 1 decision open");
  expect(out).toContain("Which builder tier builds this work item (W-001) (small, medium, large)");

  // On the swarm's rail: a card with a button for each answer.
  await open(page);
  await expect.poll(async () => (await reasonOf(page, block))?.kind ?? null, { timeout: 20_000 }).toBe("gate");
  await page.goto("/#swarm");
  const card = page.locator('.swarm-card[data-kind="gate"]');
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(card.locator(".ch b")).toHaveText("A decision waits on you");
  await expect(card.locator(".cq")).toHaveText("Which builder tier builds this work item (W-001)");
  await expect(card.locator("[data-answer-point]")).toHaveText(["Small", "Medium", "Large"]);
  await expect(card.locator('[data-answer-point="small"]')).toHaveAttribute("title", "A small builder.");
  await card.screenshot({ path: info.outputPath("rail-card.png") });

  // On a phone, from the sheet's "Needs you".
  const ctx = await browser.newContext({ ...phone, baseURL: base(), extraHTTPHeaders: { "tailscale-user-login": OWNER } });
  const mine = await ctx.newPage();
  await mine.goto("/");
  await expect.poll(async () => (await reasonOf(mine, block))?.kind ?? null, { timeout: 15_000 }).toBe("gate");
  await mine.locator(".sheet-button").click();
  const row = mine.locator(`.needs-you [data-wants="${block}"]`);
  await expect(row.locator("[data-answer-point]")).toHaveText(["Small", "Medium", "Large"]);
  await expect(row.locator("[data-approve-gate]")).toHaveCount(0);
  await row.screenshot({ path: info.outputPath("phone-sheet.png") });
  await row.locator('[data-answer-point="medium"]').tap();

  // chant's own read: answered medium, by the owner's Arugula name.
  await expect.poll(() => question("slice-tier")?.state ?? null, { timeout: 20_000 }).toBe("answered");
  expect(question("slice-tier")).toMatchObject({ answer: "medium", answeredBy: [OWNER] });
  // The attention clears, and the card says who answered.
  await expect.poll(async () => (await panesOf(page)).find((p) => p.id === block)?.attention, { timeout: 15_000 }).toBe("idle");
  const answered = (await panesOf(page)).find((p) => p.id === block)?.answered;
  expect([answered?.how, answered?.name]).toEqual(["answered medium", OWNER]);
  await ctx.close();
});

test("a gate that asks a decision point is answered on its card, and the next run walks through (#621)", async ({ page }, info) => {
  test.setTimeout(120_000);
  const ran = chant(join(ws, "delivery"), "run", "ask");
  expect(ran.status, ran.stdout + ran.stderr).toBe(3);
  const id = question("ship-ok")!.id;

  // The block's "Waiting on you": the gate, as its question.
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  const gate = shown.locator(`[data-point="${id}"]`);
  await expect(gate).toBeVisible({ timeout: 20_000 });
  await expect(gate).toHaveAttribute("data-gate", "delivery/ask/ship-ok");
  await expect(gate.locator(".ws-gate-what")).toContainText("delivery: Ship the toy now");
  await expect(gate.locator("[data-answer-point]")).toHaveText(["Yes", "No"]);
  await gate.screenshot({ path: info.outputPath("block-gate.png") });

  // Answered Yes from the swarm's card.
  await page.goto("/#swarm");
  const card = page.locator('.swarm-card[data-kind="gate"]');
  await expect(card.locator(".cq")).toHaveText("delivery: Ship the toy now", { timeout: 15_000 });
  await card.locator('[data-answer-point="true"]').click();
  await expect.poll(() => question("ship-ok")?.state ?? null, { timeout: 20_000 }).toBe("answered");
  expect(question("ship-ok")).toMatchObject({ answer: true, answeredBy: [OWNER] });

  // The op walks through its gate on that answer.
  const again = chant(join(ws, "delivery"), "run", "ask");
  expect(again.status, again.stdout + again.stderr).toBe(0);
  expect(again.stderr).toContain('Op "ask" completed');
});
