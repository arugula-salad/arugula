// A step that finishes in the person's browser (#364). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { openOutside } from "../src/shared/desktop.ts";

const URL_ = "https://control.test/#account";

/** The page as the app, or a browser, makes it: `invoke` answers the calls. */
function page({ app, invoke }) {
  const opened = [];
  globalThis.window = { open: (...a) => opened.push(a) };
  globalThis.__arugulaApp = app ? { name: "Arugula app", platform: "linux" } : undefined;
  globalThis.__TAURI__ = invoke ? { core: { invoke } } : undefined;
  return opened;
}
const settle = () => new Promise((r) => setTimeout(r, 0));

test("in the app, the app opens it and the page does not", async () => {
  const calls = [];
  const opened = page({ app: true, invoke: async (...a) => calls.push(a) });
  assert.equal(openOutside(URL_), true);
  await settle();
  assert.deepEqual(calls, [["open_outside", { url: URL_ }]]);
  assert.deepEqual(opened, []);
});

test("an app without the command falls back to window.open", async () => {
  const opened = page({ app: true, invoke: async () => Promise.reject("command open_outside not found") });
  assert.equal(openOutside(URL_), true);
  await settle();
  assert.deepEqual(opened, [[URL_, "_blank", "noopener"]]);
});

test("in a plain browser it does nothing, for the caller's window.open", async () => {
  const opened = page({ app: false, invoke: null });
  assert.equal(openOutside(URL_), false);
  await settle();
  assert.deepEqual(opened, []);
});
