// The status page (Arugula #1176): linked only when the page's hub is the
// hosted control, never for a control someone runs themselves. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { statusPage } from "../src/platform/status-page.ts";

test("hosted Arugula's hub links status.arugula.io", () => {
  assert.equal(statusPage("https://control.arugula.io"), "https://status.arugula.io");
  assert.equal(statusPage("https://control.arugula.io/"), "https://status.arugula.io");
});

test("any other hub links nothing", () => {
  for (const hub of ["https://control.example.com", "http://127.0.0.1:7801", "http://app.localhost:8080", "https://app.arugula.io", "http://control.arugula.io", "https://control.arugula.io.example.com"]) assert.equal(statusPage(hub), null, hub);
});
