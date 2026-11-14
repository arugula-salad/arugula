// Who's signed in, for the shell's blocks (Arugula #989): the app gives
// them the platform's session, as control's page gives its own, so a board
// in a machine's pane reads as the person inside the app too. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { signedInOf } from "../src/app/signed-in.ts";
import { getSignedIn, setSignedIn } from "../src/shell/signed-in.ts";

/** A platform session (platform/session.ts) the test moves along. */
function platform() {
  const listeners = new Set();
  return {
    me: null,
    state: null,
    subscribe(fn) {
      listeners.add(fn);
      return () => void listeners.delete(fn);
    },
    say(state, me = null) {
      this.state = state;
      this.me = me;
      listeners.forEach((fn) => fn());
    },
  };
}

test("loading until the platform answers, then ready as the person it names", () => {
  const p = platform();
  const s = signedInOf(p);
  assert.equal(s.phase, "loading");
  assert.equal(s.account, "");
  p.say("ready", { account: "acct_a", login: "sam", name: "Sam" });
  assert.equal(s.phase, "ready");
  assert.deepEqual([s.account, s.login, s.name], ["acct_a", "sam", "Sam"]);
  // The app holds no signed rosters.
  assert.deepEqual(s.teams, []);
});

test("signed out, or an error, isn't ready", () => {
  const p = platform();
  const s = signedInOf(p);
  p.say("signed-out");
  assert.equal(s.phase, "signed-out");
  p.say({ error: "HTTP 500" });
  assert.equal(s.phase, "error");
});

test("a block listening hears the platform's change, and stops when it's done", () => {
  const p = platform();
  const s = signedInOf(p);
  let heard = 0;
  const off = s.subscribe(() => heard++);
  p.say("ready", { account: "acct_a", login: "sam", name: "" });
  assert.equal(heard, 1);
  off();
  p.say("signed-out");
  assert.equal(heard, 1);
});

test("it holds no device-only actions: those stay control's page's", () => {
  const s = signedInOf(platform());
  assert.equal("renameDaemon" in s, false);
  assert.equal("passkeys" in s, false);
});

test("what the page sets is what the blocks read", () => {
  assert.equal(getSignedIn(), null, "nobody until the page says");
  const s = signedInOf(platform());
  setSignedIn(s);
  assert.equal(getSignedIn(), s);
  setSignedIn(null);
  assert.equal(getSignedIn(), null);
});
