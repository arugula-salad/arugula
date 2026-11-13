// An account's passkey log (DK3): the page's copy of `arugula_e2e::passkey`
// runs Rust's example files (contract/examples/logic/passkey/, written by
// crates/e2e/tests/logic/passkey.rs) case by case and gives the same
// output: each event's text and hash, and every verdict of `verify`.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { body, checkChain, eventChallenge, eventHash, proofLine, verify } from "../src/shell/e2e/passkey.ts";
import { addingPasskey } from "../src/shell/trust/adding-passkey.ts";

const file = (name) =>
  JSON.parse(readFileSync(new URL(`../../contract/examples/logic/passkey/${name}.json`, import.meta.url), "utf8"));

test("each event's text, proof line, hash and challenge are Rust's", async () => {
  const f = file("bodies");
  assert.ok(f.cases.length > 0);
  for (const c of f.cases) {
    const e = f.objects[c.input.event];
    const got = { body: body(e), proof_line: proofLine(e), hash: await eventHash(e), challenge: await eventChallenge(e) };
    assert.deepEqual(got, c.output, c.about);
  }
});

test("every verdict is Rust's", async () => {
  const f = file("verify");
  assert.ok(f.cases.length > 0);
  for (const c of f.cases) {
    const { log, account, assertion, challenge, at, origins } = c.input;
    const l = f.objects[log];
    const o = f.objects[origins];
    const v = await verify(l, account, f.objects[assertion], challenge, at, o);
    const got = v.ok
      ? { ok: true, added: v.key.seq, rp: v.rp }
      : v.refused === "chain"
        ? { ok: false, refused: "chain", event: v.event }
        : { ok: false, refused: v.refused };
    assert.deepEqual(got, c.output, c.about);
    // The chain alone says the same of a broken log.
    const chain = await checkChain(l);
    assert.equal(chain, v.ok || v.refused !== "chain" ? null : v.event, c.about);
  }
});

test("a key removed before the time checked fails, and counts just before", async () => {
  const f = file("verify");
  const at = (n) => f.cases.find((c) => c.input.assertion === "es_app" && c.input.at === n);
  assert.deepEqual(at(2999).output, { ok: true, added: 0, rp: "control.example" });
  assert.deepEqual(at(3000).output, { ok: false, refused: "not_live" });
});

// #471: "Confirm the new passkey" says what's being added: its provider (or
// that it's unknown), when it was made and the browser that made it.
test("the step-up prompt names the new passkey", () => {
  const now = Date.UTC(2026, 10, 1, 12);
  assert.equal(addingPasskey("1Password", "Chrome on Mac", now - 5_000, now), "Adding: 1Password passkey, made just now in Chrome on Mac.");
  assert.equal(addingPasskey("iCloud Keychain", "Safari on iPhone", now - 3 * 60_000, now), "Adding: iCloud Keychain passkey, made 3 minutes ago in Safari on iPhone.");
  assert.equal(
    addingPasskey(null, "Chrome on Linux", now, now),
    "Adding: a passkey from an unknown passkey provider, made just now in Chrome on Linux.",
  );
  assert.equal(addingPasskey(undefined, "A headless device (node)", now, now), "Adding: a passkey from an unknown passkey provider, made just now in a headless device (node).");
});
