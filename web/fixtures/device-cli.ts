// The approving device (device.ts) from a shell, for tests that aren't
// TypeScript (testnet/test.sh, Rust). The device lives in a state file
// between steps. Each command prints one JSON object on stdout and exits 0,
// or prints why on stderr and exits 1.
//
//   node --experimental-strip-types device-cli.ts --state FILE COMMAND ...
//
//   signin --control URL [--via FROM=TO]... [--login L] [--name N]
//                          sign in and enroll: {account, fingerprint, device, approved}
//   approve CODE [TEAM]    approve a daemon's join code, into TEAM if given
//                          (one this account owns): {device, name}
//   devices                the account's trusted devices: {devices: [{id, kind, name}]}
//   online NAME [SECS]     wait for a daemon to be online: {id, name, online, urls}
//   pane NAME MARKER [SECS] round-trip MARKER through its first pane, over
//                          control's relay: {id, name}
//   panes NAME             its panes, untouched: {id, name, panes}
//   team-create NAME       a team, this account its owner: {team}
//   team-invite TEAM [ROLE] an Ask me first invite (editor, or viewer), its
//                          one-time key's seed: {code}
//   team-accept TEAM CODE  this account asks to join with it (the seed, or
//                          the whole #askinvite= link): {}
//   team-admit TEAM        admit everyone who asked: {admitted}
//   delegate [TEAM]        as an owner, let control enroll hosted machines into
//                          the account's project, or TEAM's (M178):
//                          {project, owner, delegation}
//   undelegate [TEAM]      take its delegations back: {revoked}
//   grant-sign [TEAM]      as an owner, sign the project's standing grant for
//                          runs (M211) under its live delegation: {project, grant}
//   grant-revoke [TEAM] [stop]
//                          revoke it (`stop`: the runs going stop too):
//                          {project, grant, told}
//   hosted-entry ID        a hosted machine as the directory lists it here,
//                          proof and all (exit 1 if it isn't listed)
//   hosted-accept FILE     whether this device accepts the hosted machine whose
//                          entry (hosted-entry's output) is in FILE:
//                          {ok, why?, role?}
//   routed ID              whether control's relay routes this person to ID:
//                          {routed}
//   open NAME [SECS] [--no-wake]
//                          open it as the page does (M182: waking a hosted
//                          machine that's asleep, unless --no-wake): ms from
//                          the open to its hello and to its first pane's
//                          screen, and what control said of its box:
//                          {id, name, hello, ready, said}
//   preview ID PORT|BLOCK:SITEKEY --blocks DOMAIN:PORT [--entry FILE] [--to HOST:PORT]
//                          M186: GET / of a port on ID through its block
//                          origin, http://b-<site key>.DOMAIN:PORT, as a
//                          block's frame does. PORT opens a port block over
//                          this device's channel; BLOCK:SITEKEY tries one
//                          already open, with the Noise key from FILE
//                          (hosted-entry's output). With --to, block origins
//                          on [::1]:PORT (where *.localhost resolves) are
//                          forwarded to HOST:PORT (control as published):
//                          {block, site_key, status?, body?, refused?}

import { readFileSync } from "node:fs";
import { createServer, connect as tcp } from "node:net";
import { fingerprint } from "../src/shell/e2e/cert.ts";
import { Device } from "./device.ts";

const args = process.argv.slice(2);
const opt = (name: string): string[] => {
  const out: string[] = [];
  for (let i = args.indexOf(name); i >= 0; i = args.indexOf(name)) {
    out.push(args[i + 1]);
    args.splice(i, 2);
  }
  return out;
};
const [state] = opt("--state");
const [control] = opt("--control");
const via = Object.fromEntries(opt("--via").map((v) => [v.slice(0, v.indexOf("=")), v.slice(v.indexOf("=") + 1)]));
const [login] = opt("--login");
const [name] = opt("--name");
const noWake = args.includes("--no-wake");
if (noWake) args.splice(args.indexOf("--no-wake"), 1);
const [blocks] = opt("--blocks");
const [entry] = opt("--entry");
const [to] = opt("--to");
const [cmd, ...rest] = args;
const out = (v: unknown) => console.log(JSON.stringify(v));

async function main() {
  if (!state) throw new Error("--state FILE is required");
  if (cmd === "signin") {
    if (!control) throw new Error("signin needs --control URL");
    const d = await Device.signIn({ control, via, login, name });
    await d.save(state);
    return out({ account: d.account, fingerprint: fingerprint(d.root), device: d.keys.id, approved: d.approved });
  }
  const d = await Device.load(state);
  const secs = (s: string | undefined, def: number) => (s ? Number(s) : def) * 1000;
  const find = async (n: string) => {
    const hit = (await d.directory()).find((x) => x.id === n || x.name === n);
    if (!hit) throw new Error(`control lists no daemon ${n}`);
    return hit;
  };
  switch (cmd) {
    case "approve": {
      const c = await d.approveJoin(rest[0], rest[1]);
      return out({ device: c.device, name: c.name });
    }
    case "devices": {
      const t = await d.trusted();
      return out({ devices: [...t.values()].map((c) => ({ id: c.device, kind: c.kind, name: c.name })) });
    }
    case "online":
      return out(await d.waitOnline(rest[0], secs(rest[1], 30)));
    case "pane": {
      const box = await find(rest[0]);
      await d.roundTrip(box.id, rest[1], { timeoutMs: secs(rest[2], 15) });
      return out({ id: box.id, name: box.name });
    }
    case "panes": {
      const box = await find(rest[0]);
      return out({ id: box.id, name: box.name, panes: await d.panes(box.id) });
    }
    case "team-create":
      return out({ team: await d.createTeam(rest[0] ?? "team") });
    case "team-invite":
      return out({ code: await d.invite(rest[0], (rest[1] as "editor" | "viewer" | undefined) ?? "editor") });
    case "team-accept":
      await d.acceptInvite(rest[0], rest[1]);
      return out({});
    case "team-admit":
      return out({ admitted: await d.admitAll(rest[0]) });
    case "delegate":
      return out(await d.delegate(rest[0]));
    case "undelegate":
      return out({ revoked: await d.undelegate(rest[0]) });
    case "grant-sign":
      return out(await d.signRunGrant(rest[0] || undefined));
    case "grant-revoke":
      return out(await d.revokeRunGrant(rest[0] || undefined, rest[1] === "stop"));
    case "hosted-entry": {
      const e = await d.hostedEntry(rest[0]);
      if (!e) throw new Error(`control lists no hosted machine ${rest[0]} here`);
      return out(e);
    }
    case "hosted-accept": {
      const e = JSON.parse(readFileSync(rest[0], "utf8"));
      return out(await d.acceptHosted(e.hosted));
    }
    case "routed":
      return out({ routed: await d.routed(rest[0]) });
    case "open": {
      const box = await find(rest[0]);
      return out({ id: box.id, name: box.name, ...(await d.open(box.id, { wake: !noWake, timeoutMs: secs(rest[1], 30) })) });
    }
    case "preview": {
      if (!blocks) throw new Error("preview needs --blocks DOMAIN:PORT");
      const [domain, port] = [blocks.slice(0, blocks.lastIndexOf(":")), blocks.slice(blocks.lastIndexOf(":") + 1)];
      if (to) {
        const [h, p] = [to.slice(0, to.lastIndexOf(":")), Number(to.slice(to.lastIndexOf(":") + 1))];
        const fwd = createServer((c) => {
          const u = tcp(p, h);
          c.pipe(u).pipe(c);
          c.on("error", () => u.destroy());
          u.on("error", () => c.destroy());
        });
        await new Promise<void>((res) => fwd.listen(Number(port), "::1", res));
        fwd.unref();
      }
      const known = rest[1].includes(":") ? { id: rest[1].split(":")[0], siteKey: rest[1].split(":")[1] } : undefined;
      const noise = entry ? (JSON.parse(readFileSync(entry, "utf8")) as { hosted: { cert: { noise: string } } }).hosted.cert.noise : undefined;
      const id = known ? rest[0] : (await find(rest[0])).id;
      const r = await d.preview(id, {
        port: known ? undefined : Number(rest[1]),
        block: known,
        noise,
        origin: (key) => `http://b-${key}.${domain}:${port}`,
      });
      return out({ block: r.block, site_key: r.siteKey, status: r.status, body: r.body, refused: r.refused });
    }
    default:
      throw new Error(
        `unknown command ${cmd ?? "(none)"}: signin, approve, devices, online, pane, panes, team-create, team-invite, team-accept, team-admit, delegate, undelegate, grant-sign, grant-revoke, hosted-entry, hosted-accept, routed, open, preview`,
      );
  }
}

main().then(
  () => process.exit(0),
  (e: Error) => {
    console.error(`device: ${e.message}`);
    process.exit(1);
  },
);
