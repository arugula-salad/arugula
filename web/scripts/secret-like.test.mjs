// A to-do's note is checked as it's typed (Arugula #1468): a token, a key,
// a password said or in a link, or a long random run looks like a secret;
// plain words, a path, a commit's hash and a PR's link don't. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { looksSecret } from "../src/shared/secret-like.ts";

// Made up here, in the shapes the check knows: none is a real secret.
const fake = (head, n, c = "a1B2") => head + c.repeat(Math.ceil(n / c.length)).slice(0, n);

test("a token, a key or a password looks like a secret", () => {
  assert.equal(looksSecret(`here: ${fake("ghp_", 36)}`), "a GitHub token");
  assert.equal(looksSecret(fake("github_pat_", 40)), "a GitHub token");
  assert.equal(looksSecret(fake("sk-ant-", 40)), "an API key");
  assert.equal(looksSecret(fake("xoxb-", 20)), "a Slack token");
  assert.equal(looksSecret(`AKIA${"ABCD2345EFGH6789"}`), "an AWS key");
  assert.equal(looksSecret("-----BEGIN OPENSSH PRIVATE KEY-----"), "a private key");
  assert.equal(looksSecret("password: hunter22"), "a password");
  assert.equal(looksSecret("https://sam:hunter22@example.com/x"), "a password in a link");
  assert.equal(looksSecret(`set it to ${fake("", 40, "Zq9x")}`), "a key");
  assert.equal(looksSecret(fake("", 64, "0f3e")), "a key");
});

test("plain words, a path, a link and a commit's hash don't", () => {
  for (const s of [
    "",
    "swapped to PETG",
    "Done, it's in ~/projects/arugula-salad/arugula-release-notes-november",
    "see https://github.com/arugula-salad/arugula/pull/2929",
    "merged at 7cefb5d17a9adc95ae0bf34ac544b602aecda8e1",
    "the token is in 1Password now",
    "named it arugula-release-notes-november-2026-final",
    "couldn't: the printer needs a new nozzle",
  ])
    assert.equal(looksSecret(s), null, s);
});
