// Send feedback (shell/feedback.ts, Arugula #1217): one text box's first
// line is the title and the rest the body; a refusal reads as one line; a
// send is the daemon's `POST /api/feedback`. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { refusalLine, sendFeedback, splitFeedback } from "../src/shell/feedback.ts";

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

test("refusals are one plain line each", () => {
  assert.match(refusalLine(429, "feedback_rate_limited", 90), /Try again in 2 minutes/);
  assert.match(refusalLine(429, undefined, null), /a little while/);
  assert.match(refusalLine(403, "feedback_anonymous_off"), /Sign in, or join this machine/);
  assert.match(refusalLine(413, undefined), /too long/);
  assert.match(refusalLine(500, "boom"), /\(boom\)/);
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
  const { client, sent } = answering(200, { id: "fbk_1", shown: "F-7K2Q9X" });
  const req = { kind: "bug", title: "t", body: "", contact_ok: true, source: "person", surface: "web" };
  assert.deepEqual(await sendFeedback(client, req), { sent: { id: "fbk_1", shown: "F-7K2Q9X" }, queued: false });
  assert.deepEqual(sent, [{ method: "POST", path: "/api/feedback", req }]);
});

test("a send the daemon keeps for later is queued", async () => {
  assert.equal((await sendFeedback(answering(202, { id: "", shown: "" }).client, {})).queued, true);
  assert.equal((await sendFeedback(answering(200, { id: "fbk_2", shown: "F-2", queued: true }).client, {})).queued, true);
});

test("a refusal comes back as its line", async () => {
  const out = await sendFeedback(answering(403, { error: "feedback_anonymous_off" }).client, {});
  assert.match(out.refused, /aren't signed in/);
});
