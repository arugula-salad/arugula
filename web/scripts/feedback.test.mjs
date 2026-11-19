// Send feedback (shared/feedback.ts, shell/feedback.ts; Arugula #1217,
// #1240): one text box's first line is the title and the rest the body; a
// refusal reads as one line; the shell sends to the daemon's `POST
// /api/feedback`, the platform pages to control's with what control-wire's
// `FeedbackSubmit` holds. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { browserArch, browserOs, controlOutcome, controlSubmit, refusalLine, splitFeedback } from "../src/shared/feedback.ts";
import { sendFeedback } from "../src/shell/feedback.ts";

test("the first line with anything on it is the title, the rest the body", () => {
  assert.deepEqual(splitFeedback("Paste drops a newline\n\nIn a pane, pasting two lines\nruns the first.\n"), {
    title: "Paste drops a newline",
    body: "In a pane, pasting two lines\nruns the first.",
  });
  assert.deepEqual(splitFeedback("\n  \r\n  Just a title  "), { title: "Just a title", body: "" });
  assert.deepEqual(splitFeedback("   \n"), { title: "", body: "" });
});

test("a first line too long for a title is cut there and kept whole in the body", () => {
  const long = "x".repeat(250);
  const { title, body } = splitFeedback(`${long}\nmore`);
  assert.equal([...title].length, 200);
  assert.ok(title.endsWith("…"));
  assert.equal(body, `${long}\n\nmore`);
});

test("a refusal is the daemon's sentence; a bare code or status gets ours", () => {
  const said = "Arugula has had a lot of feedback from here lately, so it isn't taking more just now. Please try again in a minute. Thanks for writing.";
  assert.equal(refusalLine(429, said), said);
  assert.match(refusalLine(429, "feedback_rate_limited", 90), /Try again in 2 minutes/);
  assert.match(refusalLine(429, undefined, null), /a little while/);
  assert.match(refusalLine(403, "feedback_anonymous_off"), /Sign in, or join this machine/);
  assert.match(refusalLine(413, undefined), /too long/);
  assert.match(refusalLine(500, "feedback_boom"), /\(feedback_boom\)/);
});

const answering = (status, body) => {
  const sent = [];
  const client = {
    request: async (method, path, req) => {
      sent.push({ method, path, req });
      return { ok: status < 300, status, json: async () => body };
    },
  };
  return { client, sent };
};

test("a send goes to the daemon's feedback.send and says its F- id", async () => {
  const { client, sent } = answering(200, { id: "fbk_1", shown: "F-7K2Q9X", queued: false });
  const req = { kind: "bug", title: "t", body: "", contact_ok: true, source: "person", surface: "web", route: "/" };
  assert.deepEqual(await sendFeedback(client, req), { sent: { id: "fbk_1", shown: "F-7K2Q9X", queued: false } });
  assert.deepEqual(sent, [{ method: "POST", path: "/api/feedback", req }]);
});

test("a send the daemon kept for later says queued", async () => {
  assert.deepEqual(await sendFeedback(answering(200, { id: "", shown: "", queued: true }).client, {}), { sent: { id: "", shown: "", queued: true } });
});

test("a refusal comes back as its line", async () => {
  const out = await sendFeedback(answering(403, { error: "Arugula isn't taking feedback from machines that aren't joined to Arugula cloud just now." }).client, {});
  assert.match(out.refused, /aren't joined to Arugula cloud/);
});

test("control's send: the person's words, the page's context, nobody's install or stamp", () => {
  const w = { kind: "idea", title: "Boards by owner", body: "", contact_ok: true };
  assert.deepEqual(controlSubmit(w, { version: "0.71.0", os: "macos", arch: null, route: "/p/x/boards" }), {
    kind: "idea",
    title: "Boards by owner",
    body: "",
    contact_ok: true,
    source: "person",
    surface: "web",
    agent: null,
    context: { version: "0.71.0", os: "macos", arch: null, onboarding: null, route: "/p/x/boards", failed: null },
    who: { install: null, stamp: null },
  });
});

test("control's answer: its ack's F- id, or its refusal's code as our line", () => {
  assert.deepEqual(controlOutcome(200, true, { id: "fbk_1", shown: "F-1" }), { sent: { id: "fbk_1", shown: "F-1", queued: false } });
  assert.match(controlOutcome(429, false, { error: "feedback_rate_limited", difficulty: null, retry_after: 30 }).refused, /Try again in a minute/);
  assert.match(controlOutcome(413, false, { error: "feedback_too_large", difficulty: null, retry_after: null }).refused, /too long/);
  assert.match(controlOutcome(401, false, null).refused, /signed out/);
});

test("the browser's OS and processor, in control's names", () => {
  assert.equal(browserOs("macOS"), "macos");
  assert.equal(browserOs("MacIntel"), "macos");
  assert.equal(browserOs("Windows"), "windows");
  assert.equal(browserOs("Linux x86_64"), "linux");
  assert.equal(browserOs("Android"), "linux");
  assert.equal(browserOs(""), null);
  assert.equal(browserArch("arm"), "aarch64");
  assert.equal(browserArch("x86"), "x86_64");
  assert.equal(browserArch(undefined), null);
});
