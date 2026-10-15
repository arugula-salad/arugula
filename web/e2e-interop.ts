// The browser's e2e code against Rust's (`crates/e2e/examples/interop.rs`):
// the same verdicts on the same certificates, signatures each side
// accepts from the other, and a channel carrying every message kind.
//   just e2e-interop    (or: node --experimental-strip-types e2e-interop.ts)

import { execFileSync, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { evaluate, certBody, joinCode, joinMac, splitJoinCode, type Cert } from "./src/e2e/cert.ts";
import { generateKeys, signText } from "./src/e2e/keys.ts";
import { daemonNow, E2ESocket, readWelcome } from "./src/e2e/channel.ts";
import { blockRoutesLabel } from "./src/blocks/block-route.ts";
import { blockOrigin, blockSrc, credsFor, frameAllow, grantExpiry, heard, isWebKit, MAX_LIFE_MS, mint, type Framed } from "./src/blocks/through-control.ts";
import { workerDirectBlocked } from "./src/block/lna.ts";
import { follows, newInviteKey, redeemInvite, signInvite, signRedeem, signRoster, type Roster, type TeamPin } from "./src/e2e/team.ts";

const bin = process.env.INTEROP_BIN ?? "../target/debug/examples/interop";
let failed = 0;
const check = (what: string, ok: boolean, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}${detail ? `: ${detail}` : ""}`);
  if (!ok) failed++;
};

// 1. Rust's certificates, evaluated here.
const fx = JSON.parse(execFileSync(bin, ["fixtures"]).toString());
const trusted = [...(await evaluate(fx.trust, fx.certs, fx.revocations)).keys()].sort();
check("same trusted set as Rust", JSON.stringify(trusted) === JSON.stringify(fx.trusted), trusted.join(","));
check("same join code", (await joinCode(fx.daemon)) === fx.joinCode, fx.joinCode);

// 1b. The same from the vectors checked in (crates/e2e/fixtures, #200),
// which an implementation with no Rust toolchain checks itself against.
{
  const v = JSON.parse(readFileSync(new URL("../crates/e2e/fixtures/certs.json", import.meta.url), "utf8"));
  const got = [...(await evaluate(v.trust, v.certs, v.revocations)).keys()].sort();
  check("checked-in vectors: the trusted set", JSON.stringify(got) === JSON.stringify(v.trusted), got.join(","));
  check("checked-in vectors: the join code", (await joinCode(v.daemon)) === v.joinCode, v.joinCode);
}

// 1c. The join secret's MAC (#760): the same answer as Rust's
// `the_join_mac_is_fixed` (crates/e2e/src/cert.rs), dashes or none.
{
  const c = { v: 1, account: "", device: "fedcba9876543210", kind: "daemon", name: "box", noise: "aa", sign: "bb", created: 0, approver: "", sig: "" } as Cert;
  const trust = { account: "a1", root: "0123456789abcdef" };
  const want = "21ec725fbc58f0e2aac0c21b84b6c9f99ca6f4a15bf48f5cf2d98598eaab135b";
  check("the join MAC", (await joinMac("ABCD-EFGH-JKMN-PQRS", trust, c)) === want);
  check("the join MAC without dashes", (await joinMac("ABCDEFGHJKMNPQRS", trust, c)) === want);
  const split = splitJoinCode("abcde-fghjk.abcd efgh jkmn pqrs");
  check("a whole code splits", split?.code === "ABCDE-FGHJK" && split.secret === "ABCD-EFGH-JKMN-PQRS", JSON.stringify(split));
}

// 2. Certificates signed here, evaluated by Rust.
const root = await generateKeys();
const phone = await generateKeys();
const mk = async (k: typeof root, by: typeof root, name: string, created: number): Promise<Cert> => {
  const c: Cert = { v: 1, account: "acct2", device: k.id, kind: "browser", name, noise: k.noisePub, sign: k.signPub, created, approver: by.id, sig: "" };
  c.sig = await signText(by, certBody(c));
  return c;
};
const certs = [await mk(root, root, "laptop ✓", 1), await mk(phone, root, "phone", 2)];
const rust = JSON.parse(execFileSync(bin, ["check"], { input: JSON.stringify({ trust: { account: "acct2", root: root.id }, certs }) }).toString());
check("Rust trusts what we signed", JSON.stringify(rust) === JSON.stringify([root.id, phone.id].sort()));

// 3. A team roster and a presigned invite signed here: Rust and this code
// agree on each version, and on a tampered one.
{
  const alice = await generateKeys();
  const bob = await generateKeys();
  const self = async (k: typeof alice, account: string): Promise<Cert> => {
    const c: Cert = { v: 1, account, device: k.id, kind: "browser", name: account, noise: k.noisePub, sign: k.signPub, created: 1, approver: k.id, sig: "" };
    c.sig = await signText(k, certBody(c));
    return c;
  };
  const tc = { alice: [[await self(alice, "alice")], []], bob: [[await self(bob, "bob")], []] } as Record<string, [Cert[], []]>;
  const pin: TeamPin = { team: "0123456789abcdef", founder: "alice", founder_root: alice.id };
  const v1 = await signRoster(
    { v: 1, team: pin.team, name: "Acme", version: 1, at: 1, members: [{ account: "alice", root: alice.id, role: "owner", name: "alice" }] },
    alice,
  );
  const { seed, key } = await newInviteKey();
  const inv = await signInvite({ team: pin.team, role: "editor", expires: Date.now() + 86_400_000, key }, alice);
  const v2 = await redeemInvite(v1, inv, seed, { account: "bob", root: bob.id, name: "bob" });
  const both = async (what: string, prev: Roster | null, r: Roster, want: boolean) => {
    const rs = execFileSync(bin, ["roster"], { input: JSON.stringify({ pin, prev, roster: r, certs: tc }) }).toString().trim() === "true";
    const ts = await follows(r, prev, pin, tc);
    check(what, rs === want && ts === want, `rust ${rs}, ts ${ts}`);
  };
  await both("team v1 follows", null, v1, true);
  await both("a presigned invite redeemed here checks out in Rust", v1, v2, true);
  const raised = await signRedeem({ ...v2, members: v2.members.map((m) => (m.account === "bob" ? { ...m, role: "owner" as const } : m)) }, seed);
  await both("a raised role doesn't", v1, raised, false);
  await both("nor one the invitee's device signs", v1, await signRoster(v2, bob), false);
  const later = await signRoster({ ...v1, at: Date.now() + 60_000 }, alice);
  await both("nor one backdated before the version it follows", later, await signRedeem({ ...(await redeemInvite(later, inv, seed, { account: "bob", root: bob.id, name: "bob" })), at: 2 }, seed), false);
  // Shapes Rust refuses before checking a signature, refused here too.
  const odd = (r: Roster, f: (x: any) => void) => { const x = structuredClone(r) as any; f(x); return x as Roster; };
  await both("nor an expiry written as a string", v1, await signRedeem(odd(v2, (x) => (x.spent[0].expires = String(x.spent[0].expires))), seed), false);
  await both("nor a spent key with a space in it", v1, await signRedeem(odd(v2, (x) => (x.spent[0].key += " x")), seed), false);
  await both("nor a member name with a space in it", v1, await signRedeem(odd(v2, (x) => (x.members[1].name = "bob b")), seed), false);
  await both("nor the same invite again", v2, await redeemInvite(v2, inv, seed, { account: "carol", root: bob.id, name: "carol" }), false);
}

// 3b. Block grants (M50): what control's page mints, Rust's Grant::check
// takes, and the frame contract's checks on who's asking.
{
  const dev = await generateKeys();
  const daemon = { id: "fedcba9876543210", noise: "aa".repeat(32) };
  const now = 1_800_000_000_000;
  const c = await mint({ keys: dev, daemon, block: "42", now, route: "relay" });
  const rust = (over: Record<string, unknown> = {}) =>
    execFileSync(bin, ["grant"], {
      input: JSON.stringify({ grant: c.grant, daemon: daemon.id, key: c.key.pub, now, signer: { id: dev.id, sign: dev.signPub }, block: "42", ...over }),
    })
      .toString()
      .trim();
  check("a grant minted here passes Rust's check", rust() === "ok", rust());
  check("its expiry is a few minutes past the daemon's now", c.grant.expires > now && c.grant.expires - now <= MAX_LIFE_MS && c.grant.expires === grantExpiry(now));
  check("its key is the one handed over", c.key.pub.length === 64 && c.grant.key === c.key.pub && c.key.pkcs8.byteLength > 32);
  check("…and expires by the daemon's clock", rust({ now: c.grant.expires }) === "expired");
  check("…for that block only", rust({ block: "43" }) === "no-block");
  check("…for that key only", rust({ key: "bb".repeat(32) }) === "other-key");
  check("…signed by this device", rust({ signer: { id: dev.id, sign: root.signPub } }) === "signature");
  const stretched = { ...c.grant, expires: c.grant.expires + 60_000 };
  check("a stretched grant fails its signature", rust({ grant: stretched }) === "signature");
  // The daemon's now comes from its clock and how long ago that was here,
  // never this device's wall clock.
  check("the daemon's now, from its clock", daemonNow({ now, at: 1000 }, 4000) === now + 3000 && daemonNow({ now, at: 5000 }, 4000) === now);
  const w = new TextEncoder();
  check("message 2's clock is read", readWelcome(w.encode('{"now":5}'), 7).clock?.now === 5 && readWelcome(new Uint8Array(), 0).clock === undefined);
  check("message 2's refusal is read", readWelcome(w.encode('{"ok":false,"refused":"expired","message":"the grant expired"}'), 0).refused === "the grant expired");
  // Route: the daemon directly where this page reached it so, except WebKit.
  const ua = {
    chrome: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
    safari: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    ios: "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/141.0 Mobile/15E148 Safari/604.1",
    gtk: "Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Safari/605.1.15",
    firefox: "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0",
  };
  check("WebKit is Safari, iOS (any browser) and WebKitGTK", isWebKit(ua.safari) && isWebKit(ua.ios) && isWebKit(ua.gtk) && isWebKit(ua.chrome, true));
  check("…and not Chrome or Firefox", !isWebKit(ua.chrome) && !isWebKit(ua.firefox));
  const client = (direct: string | null) => ({ blockDomain: () => "arugula.run", e2e: { daemon, keys: dev }, daemonClock: () => ({ now, at: performance.now() }), directE2E: () => direct });
  const direct = "wss://box.example.ts.net/e2e";
  check("direct in Chrome when the page is", (await credsFor(client(direct), "42", ua.chrome, false)).route === direct);
  check("the relay in WebKit", (await credsFor(client(direct), "42", ua.safari, false)).route === "relay" && (await credsFor(client(direct), "42", ua.ios, false)).route === "relay");
  check("the relay when the page is relayed", (await credsFor(client(null), "42", ua.chrome, false)).route === "relay");
  const noClock = { ...client(null), daemonClock: () => undefined };
  check("no grant without the daemon's clock", await credsFor(noClock, "42", ua.chrome, false).then(() => false, () => true));
  // Origins and addresses.
  const origin = blockOrigin("0123456789abcdef0123456789abcdef", "arugula.run", { protocol: "https:", port: "" });
  check("a block's origin", origin === "https://b-0123456789abcdef0123456789abcdef.arugula.run", origin);
  check("…on a local control's port", blockOrigin("ab", "blocks.localhost", { protocol: "http:", port: "7690" }) === "http://b-ab.blocks.localhost:7690");
  check("a port's address moves to it", blockSrc("http://b-4-k.localhost:7701/app?x=1#h", origin) === `${origin}/app?x=1#h`);
  check("…and a path", blockSrc("/", origin) === `${origin}/`);
  const payload = '[["openFile","vscode-remote://b-4-k.localhost:7701/src/a%20b.rs:3"],["gotoLineMode","true"]]';
  const ed = new URL(blockSrc(`http://b-4-k.localhost:7701/?workspace=%2Fw%2Fx.code-workspace&payload=${encodeURIComponent(payload)}`, origin));
  check(
    "an editor's file opens on the block origin's remote",
    ed.origin === origin && ed.searchParams.get("workspace") === "/w/x.code-workspace" && ed.searchParams.get("payload") === payload.replace("b-4-k.localhost:7701", new URL(origin).host),
    ed.searchParams.get("payload") ?? "",
  );
  const noHost = new URL(blockSrc(`/?folder=%2Fr&payload=${encodeURIComponent('[["openFile","vscode-remote:///r/a.rs"]]')}`, origin));
  check("…a daemon with no block site of its own too", noHost.searchParams.get("payload") === `[["openFile","vscode-remote://${new URL(origin).host}/r/a.rs"]]`);
  // Who's answered: a frame this page made, at the origin it gave it.
  const winA = { name: "a" };
  const winB = { name: "b" };
  const fA: Framed = { frame: { contentWindow: winA }, origin, creds: async () => c, failed: () => {}, routed: () => {} };
  const fB: Framed = { frame: { contentWindow: winB }, origin: "https://b-ffff.arugula.run", creds: async () => c, failed: () => {}, routed: () => {} };
  const msg = (data: unknown, source: unknown = winA, o = origin) => heard([fA, fB], { source, origin: o, data });
  const hello = { arugula: "arugula-block", v: 1, ask: "hello" };
  check("a frame's hello is answered, for that frame", msg(hello)?.from === fA);
  check("…and its renew", msg({ ...hello, ask: "renew" })?.from === fA);
  check("…and its failure heard", (msg({ arugula: "arugula-block", v: 1, failed: "no service workers" })?.said as { failed?: string }).failed === "no service workers");
  check("a window this page didn't frame isn't", msg(hello, { name: "stranger" }) === null);
  check("nor a frame at another origin than it was given", msg(hello, winA, "https://evil.example") === null);
  check("nor one frame claiming another block's origin", msg(hello, winA, fB.origin) === null);
  check("another block's frame gets its own", msg(hello, winB, fB.origin)?.from === fB);
  check("nor an untagged or other-version message", msg({ ask: "hello" }) === null && msg({ ...hello, v: 2 }) === null && msg({ ...hello, ask: "steal" }) === null);
  check("nor one with no source", msg(hello, null) === null);
  // Which way a block's channel went (PR 5): the same checks.
  const routed = { arugula: "arugula-block", v: 1, route: "relayed" };
  const said = (m: unknown, source: unknown = winA, o = origin) => (msg(m, source, o)?.said as { route?: string } | undefined)?.route;
  check("a frame's route is heard, for that frame", said(routed) === "relayed" && msg(routed)?.from === fA && said({ ...routed, route: "direct" }) === "direct");
  check("…and another block's for its own", msg(routed, winB, fB.origin)?.from === fB);
  check("a route from a window this page didn't frame isn't", msg(routed, { name: "stranger" }) === null && msg(routed, null) === null);
  check("…nor from another origin, or another block's", msg(routed, winA, "https://evil.example") === null && msg(routed, winA, fB.origin) === null);
  check(
    "…nor an untagged, other-version or unknown route",
    msg({ route: "relayed" }) === null && msg({ ...routed, v: 2 }) === null && msg({ ...routed, route: "teleport" }) === null && msg({ ...routed, route: 1 }) === null,
  );
  // …and why not direct, and whether it's the pages' own channel (#849).
  const why = (m: unknown) => msg(m)?.said as { route?: string; why?: string; ws?: boolean } | undefined;
  check("a route's reason is heard", why({ ...routed, why: "couldn't connect (after 3 ms)" })?.why === "couldn't connect (after 3 ms)" && why(routed)?.why === undefined);
  check("…cut short, and only a string", (why({ ...routed, why: "x".repeat(900) })?.why ?? "").length === 300 && why({ ...routed, why: 7 })?.why === undefined);
  check("the pages' own channel is told apart", why({ ...routed, route: "direct", ws: true })?.ws === true && why({ ...routed, ws: "yes" })?.ws === undefined);
  // Chrome's Local Network Access: the frame is allowed it, its worker
  // can't have it from Chrome 147 (WebSockets checked) on.
  check("a frame is allowed the local network", frameAllow(undefined) === "local-network; local-network-access" && frameAllow("clipboard-read") === "clipboard-read; local-network; local-network-access");
  const chrome = (v: number) => ua.chrome.replace("141.0.0.0", `${v}.0.0.0`);
  check("Chrome's worker skips a direct route", workerDirectBlocked(chrome(147)) !== null && workerDirectBlocked(chrome(153)) !== null);
  check("…not before 147, nor Firefox or WebKit", workerDirectBlocked(chrome(146)) === null && workerDirectBlocked(ua.firefox) === null && workerDirectBlocked(ua.safari) === null);
  check("the host menu sums blocks' routes", blockRoutesLabel([]) === null && blockRoutesLabel(["relayed", "relayed"]) === "Blocks: relayed");
  check(
    "…in the button's words",
    blockRoutesLabel(["relayed", "direct", "own site"]) === "Blocks: 1 direct, 1 relayed, 1 on its own block site" && blockRoutesLabel(["own site"]) === "Blocks: the machine's own block site",
    String(blockRoutesLabel(["relayed", "direct", "own site"])),
  );
}

// 4. A channel to a Rust responder.
const child = spawn(bin, ["responder", "127.0.0.1:0"]);
const [addr, id, noise] = await new Promise<string[]>((r) => child.stdout.once("data", (d) => r(d.toString().trim().split(" "))));
try {
  const sock = await E2ESocket.connect([{ url: `ws://${addr}/`, timeoutMs: 2000 }], { id, noise }, phone);
  const texts: string[] = [];
  const bins: Uint8Array[] = [];
  sock.onText = (t) => texts.push(t);
  sock.onBinary = (b) => bins.push(b);
  sock.start();
  check("the daemon's clock came in message 2", !!sock.clock && Math.abs(sock.clock.now - Date.now()) < 60_000, JSON.stringify(sock.clock));
  sock.sendText('{"type":"attach"}');
  const big = Uint8Array.from({ length: 100_000 }, (_, i) => i % 251);
  sock.sendBinary(big);
  const res = await sock.request("POST", "/api/run?x=1", { cmd: "ls" });
  await new Promise((r) => setTimeout(r, 200));
  check("text echoed", texts[0] === '{"type":"attach"}');
  check("100 KB binary echoed in chunks", bins[0]?.length === big.length && bins[0].every((b, i) => b === big[i]));
  const j = res.json<{ method: string; path: string; len: number }>();
  check("request answered", res.status === 201 && j.method === "POST" && j.path === "/api/run?x=1" && j.len === 12, JSON.stringify(j));
  sock.close();
  // A wrong daemon id (the relay splicing us elsewhere) fails the handshake.
  const wrong = await E2ESocket.connect([{ url: `ws://${addr}/`, timeoutMs: 2000 }], { id: "0000000000000000", noise }, phone).then(
    () => false,
    () => true,
  );
  check("wrong daemon refused", wrong);
} finally {
  child.kill();
}
process.exit(failed ? 1 : 0);
