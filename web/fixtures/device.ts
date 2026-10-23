// A headless approving device: what a signed-in browser or phone does on
// control, without a browser, for tests that need someone to approve
// something. It signs in through control's GitHub sign-in (against
// fakes.ts's GitHub), enrolls its device keys, approves join codes, checks
// the account's devices, and reaches a daemon end to end, through control's
// relay or directly. It uses the web client's own e2e code (src/e2e), so it
// is the browser's behaviour, not a copy of it.
//
//   const me = await Device.signIn({ control: "http://127.0.0.1:7690", login: "alice" });
//   await me.approveJoin("ABCDE-FGHIJ.KLMN-PQRS-TVWX-YZ01");
//   const box = await me.waitOnline("box");
//   await me.roundTrip(box.id, "MARKER");
//
// `device-cli.ts` is the same from a shell, keeping the device in a file.
// docs/testing.md describes both.

import { readFileSync, writeFileSync } from "node:fs";
import { certBody, deviceMac, evaluate, joinCode, joinMac, revocationBody, splitJoinCode, unhex, type Cert, type Revocation } from "../src/shell/e2e/cert.ts";
import { daemonNow, E2ESocket } from "../src/shell/e2e/channel.ts";
import type { WakeSaid } from "../src/shell/e2e/relaymux.ts";
import { mint } from "../src/shell/blocks/through-control.ts";
import { generateKeys, signText, type DeviceKeys } from "../src/shell/e2e/keys.ts";
import { acceptHosted, signDelegation, signRevoke, type Delegation, type DelegationRevoke, type HostedProof } from "../src/shell/e2e/hosted.ts";
import { signGrant, signGrantRevoke, type ProjectGrant } from "../src/shell/e2e/run-grant.ts";
import { approvalBody, signApproval, DEFAULT_LIFE_MS } from "../src/shell/e2e/deploy.ts";
import { askBody, askOk, inviteKey, newInviteKey, signAskInvite, signRoster, teamJoinBody, word, type AccountCerts, type AskInvite, type Roster, type TeamPin, type TeamRole } from "../src/shell/e2e/team.ts";

const subtle = globalThis.crypto.subtle;

/** The Origin a block's channel sends (`preview`): Node's WebSocket sends
 * none, and control's relay on a block origin wants the block's own. The
 * block channel's code takes the WebSocket class when it's first imported,
 * so this one stays in place, and a socket made with headers of its own
 * (`connect`, `routed`) keeps them. */
const blockHeaders: { origin?: string } = {};
{
  const Native = globalThis.WebSocket;
  globalThis.WebSocket = class extends Native {
    constructor(u: string | URL, p?: string | string[]) {
      super(u, (p ?? (blockHeaders.origin ? { headers: { origin: blockHeaders.origin } } : undefined)) as string[]);
    }
  } as typeof WebSocket;
}
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export interface DeviceOptions {
  /** Control's public URL, as it knows itself (its `--public-url`). Sent as
   * the Origin, and where requests go unless `via` says otherwise. */
  control: string;
  /** Where requests for a URL prefix really go, for a control or fake
   * GitHub whose public URL the test can't reach as written (a container's
   * address): `{ "http://10.229.80.10:8080": "http://127.0.0.1:22980" }`. */
  via?: Record<string, string>;
}

export interface SignInOptions extends DeviceOptions {
  /** Who the fake GitHub signs in [default: "stranger"]. */
  login?: string;
  /** This device's name on the account [default: "laptop"]. */
  name?: string;
  /** `browser` (the default) or `cli`. */
  kind?: Cert["kind"];
}

/** A daemon as control's directory lists it. */
export interface Listed {
  id: string;
  name: string;
  online: boolean;
  urls: string[];
}

/** What `save` writes and `load` reads: the keys (private halves as JWK),
 * the session and the pinned account. Test data only: it holds a usable
 * device key. */
interface Saved {
  control: string;
  via: Record<string, string>;
  cookie: string;
  account: string;
  login: string;
  root: string;
  name: string;
  approved: boolean;
  keys: { id: string; noisePub: string; signPub: string; noise: JsonWebKey; sign: JsonWebKey };
  /** A software passkey (M219's testnet): its credential id, the RP id it
   * was made for, and its Ed25519 key. */
  passkey?: { id: string; rpId: string; key: JsonWebKey; pub: string };
}

/** base64url, no padding, as WebAuthn writes it. */
const b64url = (b: Uint8Array) => Buffer.from(b).toString("base64url");
const sha256 = async (b: Uint8Array | string) =>
  new Uint8Array(await subtle.digest("SHA-256", typeof b === "string" ? new TextEncoder().encode(b) : b));

/** CBOR, the little a `none` attestation needs: unsigned and negative
 * integers, byte and text strings, and maps (as pairs, in order). */
type Cbor = number | string | Uint8Array | [Cbor, Cbor][];
function cbor(v: Cbor): Uint8Array {
  const head = (major: number, n: number): number[] =>
    n < 24 ? [(major << 5) | n] : n < 256 ? [(major << 5) | 24, n] : [(major << 5) | 25, n >> 8, n & 255];
  if (typeof v === "number") return new Uint8Array(v >= 0 ? head(0, v) : head(1, -1 - v));
  if (typeof v === "string") {
    const b = new TextEncoder().encode(v);
    return new Uint8Array([...head(3, b.length), ...b]);
  }
  if (v instanceof Uint8Array) return new Uint8Array([...head(2, v.length), ...v]);
  const parts = v.flatMap(([k, x]) => [...cbor(k), ...cbor(x)]);
  return new Uint8Array([...head(5, v.length), ...parts]);
}

export class Device {
  readonly control: string;
  readonly via: Record<string, string>;
  keys: DeviceKeys;
  /** The session cookie (`ilg_session=…`). */
  cookie = "";
  /** The account's id on control, and the GitHub login it signed in as. */
  account = "";
  login = "";
  /** The account root this device pinned when it enrolled: what every
   * certificate it accepts has to chain back to. */
  root = "";
  name = "";
  /** A software passkey, once `addPasskey` made one. */
  passkey?: { id: string; rpId: string; key: CryptoKey; pub: string };
  /** Whether the account trusts this device yet (the first one is trusted
   * on enrollment; later ones wait for an approval). */
  approved = false;

  private constructor(opts: DeviceOptions, keys: DeviceKeys) {
    this.control = opts.control.replace(/\/$/, "");
    this.via = opts.via ?? {};
    this.keys = keys;
  }

  /** Sign in, make device keys and enroll them. The account's first device
   * signs itself and is trusted at once; a later one is left waiting for
   * `approveDevice` from a trusted one. */
  static async signIn(opts: SignInOptions): Promise<Device> {
    const d = new Device(opts, await generateKeys(true));
    await d.session(opts.login ?? "stranger");
    d.name = opts.name ?? "laptop";
    const me = await d.api<{ account: string; login: string }>("/api/me");
    d.account = me.account;
    d.login = me.login;
    const first = await d.api<{ trust: { root: string } | null }>("/api/devices");
    const kind = opts.kind ?? "browser";
    // The first device signs its own certificate; any other asks.
    const own = !first.trust;
    const c = own ? await d.sign(d.keys, kind, d.name) : { ...(await d.sign(d.keys, kind, d.name)), approver: "", sig: "" };
    const r = await d.api<{ approved: boolean }>("/api/devices", { cert: c });
    d.approved = r.approved;
    d.root = own ? d.keys.id : first.trust!.root;
    return d;
  }

  /** Sign in again with the same keys (a new session): after control
   * restarted with a fresh database, say, or to switch who's signed in. */
  async session(login = this.login || "stranger"): Promise<void> {
    let url = `${this.control}/auth/github?next=/`;
    const jar: Record<string, string> = {};
    for (let i = 0; i < 6; i++) {
      // The fake GitHub signs in whoever the authorize URL names.
      const u = new URL(url);
      if (u.pathname === "/login/oauth/authorize") u.searchParams.set("login", login);
      const res = await fetch(this.reach(u.href), {
        redirect: "manual",
        headers: { cookie: Object.entries(jar).map(([k, v]) => `${k}=${v}`).join("; ") },
      });
      for (const c of res.headers.getSetCookie()) {
        const [kv] = c.split(";");
        const at = kv.indexOf("=");
        jar[kv.slice(0, at)] = kv.slice(at + 1);
      }
      const loc = res.headers.get("location");
      if (!loc) {
        if (!jar.ilg_session) throw new Error(`signing in as ${login}: ${res.status} ${(await res.text()).slice(0, 200)}`);
        break;
      }
      url = new URL(loc, url).href;
    }
    if (!jar.ilg_session) throw new Error(`signing in as ${login}: no session cookie`);
    this.cookie = `ilg_session=${jar.ilg_session}`;
  }

  /** A URL as this test can reach it (`via`). */
  reach(url: string): string {
    for (const [from, to] of Object.entries(this.via)) if (url.startsWith(from)) return to + url.slice(from.length);
    return url;
  }

  /** Control's API as this device's session: GET without a body, POST with
   * one. Throws with control's status and error on anything but 2xx. */
  async api<T>(path: string, body?: unknown, method?: string): Promise<T> {
    const res = await fetch(this.reach(this.control + path), {
      method: method ?? (body === undefined ? "GET" : "POST"),
      headers: { cookie: this.cookie, origin: this.control, ...(body === undefined ? {} : { "content-type": "application/json" }) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const j = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(`${path}: ${res.status} ${JSON.stringify(j)}`);
    return j as T;
  }

  /** A certificate for `k`, signed by `by` (this device unless given). */
  async sign(k: DeviceKeys, kind: Cert["kind"], name: string, by: DeviceKeys = this.keys): Promise<Cert> {
    const c: Cert = { v: 1, account: this.account, device: k.id, kind, name, noise: k.noisePub, sign: k.signPub, created: Date.now(), approver: by.id, sig: "" };
    c.sig = await signText(by, certBody(c));
    return c;
  }

  /** Approve another device waiting on the account (one that signed in
   * and enrolled after the first). */
  async approveDevice(other: Device): Promise<void> {
    await this.api(`/api/devices/${other.keys.id}/approve`, { cert: await this.sign(other.keys, "browser", other.name) });
    other.approved = true;
  }

  /** Approve a browser waiting on the account, from the certificate it
   * asked with (its page's `control.request`), as the "New device?"
   * prompt does. With the code it shows (#761, its page's
   * `control.deviceCode`, `LOOKUP.SECRET`), the lookup must be its keys'
   * code and the approval carries the MAC under the secret, as the prompt
   * sends it; without one, it's an approval from a page from before codes,
   * and the browser asks its person to check the account instead. */
  async approveRequest(asked: Cert, code?: string | null): Promise<void> {
    const k = { id: asked.device, noisePub: asked.noise, signPub: asked.sign } as DeviceKeys;
    let mac: string | undefined;
    if (code) {
      const parts = splitJoinCode(code);
      if (!parts?.secret) throw new Error(`${code} isn't a device's code`);
      const want = await joinCode(asked);
      if (want !== parts.code) throw new Error(`the browser waiting has a key whose code is ${want}, not ${parts.code}`);
      mac = await deviceMac(parts.secret, { account: this.account, root: this.root }, asked);
    }
    await this.api(`/api/devices/${asked.device}/approve`, { cert: await this.sign(k, asked.kind, asked.name), ...(mac ? { mac } : {}) });
  }

  /** Approve a daemon's join code (what `arugulad join` prints after
   * `#join=`, the whole of it) into this account, as the approve page does:
   * the code must be the one the waiting daemon's key gives. With the join
   * secret after it (#760, `CODE.SECRET`), the approval carries the MAC
   * under it, and the daemon needs nothing else; with the code alone, the
   * daemon is an older one, or joins with `--account`. Returns its
   * certificate. With `team` (one this account owns), into that team,
   * signed as the page signs it. */
  async approveJoin(whole: string, team?: string): Promise<Cert> {
    const parts = splitJoinCode(whole);
    if (!parts) throw new Error(`${whole} isn't a join code`);
    const { code, secret } = parts;
    const shown = await this.api<{ cert: Cert }>(`/api/joins/${code}`);
    const want = await joinCode(shown.cert);
    if (want !== code) throw new Error(`the daemon waiting on ${code} has a key whose code is ${want}`);
    const c: Cert = { ...shown.cert, account: this.account, approver: this.keys.id, sig: "" };
    c.sig = await signText(this.keys, certBody(c));
    let team_sig: string | null = null;
    if (team) team_sig = await signText(this.keys, teamJoinBody(c.device, (await this.team(team)).pin));
    const mac = secret ? await joinMac(secret, { account: this.account, root: this.root }, shown.cert) : null;
    await this.api(`/api/joins/${code}/approve`, { cert: c, team: team ?? null, team_sig, ...(mac ? { mac } : {}) });
    return c;
  }

  /** Remove a device or machine from the account, as the page's Remove
   * button does: a revocation this device signs. */
  async revoke(id: string): Promise<void> {
    const r: Revocation = { v: 1, account: this.account, device: id, at: Date.now(), by: this.keys.id, sig: "" };
    r.sig = await signText(this.keys, revocationBody(r));
    await this.api("/api/revocations", { revocation: r });
  }

  // ---- teams (M19), as the page's owner does them

  /** A team this account is in, as control lists it. */
  async team(id: string): Promise<{
    team: string;
    pin: TeamPin;
    roster: Roster;
    certs: AccountCerts;
    requests: { account: string; root: string; name: string; role: TeamRole; invite?: AskInvite; proof?: string }[];
  }> {
    const { teams } = await this.api<{ teams: Awaited<ReturnType<Device["team"]>>[] }>("/api/teams");
    const t = teams.find((x) => x.team === id);
    if (!t) throw new Error(`this account isn't in team ${id}`);
    return t;
  }

  /** A new team with this account its owner. Returns its id. */
  async createTeam(name: string): Promise<string> {
    const team = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, "0")).join("");
    const me = { account: this.account, root: this.root, name: word(this.login || this.name), role: "owner" as const };
    const roster = await signRoster({ v: 1, team, name, version: 1, at: Date.now(), members: [me] }, this.keys);
    await this.api("/api/teams", { roster });
    return team;
  }

  /** An Ask me first invite (the owner admits whoever accepts it): the
   * seed of its one-time key, what `#askinvite=TEAM.SEED` carries (#762). */
  async invite(team: string, role: TeamRole = "editor"): Promise<string> {
    const { seed, key } = await newInviteKey();
    const made = Date.now();
    const ask = await signAskInvite({ team, role, made, expires: made + 7 * 86_400_000, key }, this.keys);
    await this.api(`/api/teams/${team}/invites`, { role, ask });
    return seed;
  }

  /** Accept an invite by its seed (or a whole `#askinvite=` link): this
   * account asks to join, with the one-time key's proof. */
  async acceptInvite(team: string, seed: string): Promise<void> {
    const k = await inviteKey(/#askinvite=[0-9a-f]+\.([0-9a-f]+)/.exec(seed)?.[1] ?? seed);
    const proof = await k.sign(askBody({ team, key: k.key }, this.account, this.root));
    await this.api(`/api/invites/${team}/${k.key}/accept`, { proof });
  }

  /** Admit everyone who asked: the next roster, signed by this owner. */
  async admitAll(team: string): Promise<number> {
    const t = await this.team(team);
    // As an owner's page does (#762): only those the link vouches for.
    const ok: (typeof t.requests[number] & { invite: AskInvite })[] = [];
    for (const r of t.requests) if (r.invite && (await askOk(t.roster, t.certs, r))) ok.push({ ...r, invite: r.invite });
    if (!ok.length) return 0;
    const asked = new Set(ok.map((r) => r.account));
    const members = [
      ...t.roster.members.filter((m) => !asked.has(m.account)),
      ...ok.map((r) => ({ account: r.account, root: r.root, role: r.invite.role, name: word(r.name) })),
    ];
    const next = await signRoster({ v: 1, team, name: t.roster.name, version: t.roster.version + 1, at: Date.now(), members }, this.keys);
    await this.api(`/api/teams/${team}/roster`, { roster: next });
    return ok.length;
  }

  /** The account's devices and machines that chain back to the root this
   * device pinned, by device id. */
  async trusted(): Promise<Map<string, Cert>> {
    const all = await this.api<{ trust: { account: string; root: string }; certs: Cert[]; revocations?: Revocation[] }>("/api/devices");
    return evaluate({ account: this.account, root: this.root }, all.certs, all.revocations ?? []);
  }

  // ---- hosted machines (M178), as an owner's page and a member's do

  /** This account's project: its own, or a team's (by team id). */
  async project(team?: string): Promise<{ id: string; owner: string }> {
    const { projects } = await this.api<{ projects: { id: string; team: { id: string } | null }[] }>("/api/projects");
    const p = projects.find((x) => (team ? x.team?.id === team : !x.team));
    if (!p) throw new Error(`no project for ${team ? `team ${team}` : "this account"}`);
    return { id: p.id, owner: team ? `team:${team}` : `account:${this.account}` };
  }

  /** Register a software passkey on the account (M219's testnet: an
   * authenticator that verifies the user, Ed25519, `none` attestation),
   * through control's own WebAuthn routes, as a browser would. */
  async addPasskey(): Promise<{ id: string }> {
    const opts = await this.api<{ challenge: string; rp: { id: string } }>("/auth/passkey/register", {});
    const pair = (await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])) as CryptoKeyPair;
    const pub = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
    const cred = crypto.getRandomValues(new Uint8Array(16));
    const cose = cbor([[1, 1], [3, -8], [-1, 6], [-2, pub]]);
    const flags = 0x01 | 0x04 | 0x40;
    const authData = new Uint8Array([
      ...(await sha256(opts.rp.id)), flags, 0, 0, 0, 0, ...new Uint8Array(16), 0, cred.length, ...cred, ...cose,
    ]);
    const clientData = JSON.stringify({ type: "webauthn.create", challenge: opts.challenge, origin: this.control });
    const att = cbor([["fmt", "none"], ["attStmt", []], ["authData", authData]]);
    const id = b64url(cred);
    // This browser device signs for the new passkey, as the page does.
    const body = `arugula add passkey device v1\naccount ${this.account}\ncredential ${id}\nchallenge ${opts.challenge}\n`;
    await this.api("/auth/passkey/register/finish", {
      id,
      clientDataJSON: b64url(new TextEncoder().encode(clientData)),
      attestationObject: b64url(att),
      device: { id: this.keys.id, sig: await signText(this.keys, body) },
    });
    this.passkey = { id, rpId: opts.rp.id, key: pair.privateKey, pub: Buffer.from(pub).toString("hex") };
    return { id };
  }

  /** The passkey's assertion over `challenge` (base64url), user verified. */
  async assert(challenge: string): Promise<{ id: string; clientDataJSON: string; authenticatorData: string; signature: string }> {
    if (!this.passkey) throw new Error("this device has no passkey: passkey-add first");
    const data = new Uint8Array([...(await sha256(this.passkey.rpId)), 0x05, 0, 0, 0, 0]);
    const clientData = new TextEncoder().encode(JSON.stringify({ type: "webauthn.get", challenge, origin: this.control }));
    const msg = new Uint8Array([...data, ...(await sha256(clientData))]);
    const sig = new Uint8Array(await subtle.sign("Ed25519", this.passkey.key, msg));
    return { id: this.passkey.id, clientDataJSON: b64url(clientData), authenticatorData: b64url(data), signature: b64url(sig) };
  }

  /** As an owner, make an environment (M217) in the account's project, or
   * TEAM's, with `driver` and the names of its secrets. */
  async environment(name: string, driver: string, secrets: string[], team?: string): Promise<{ environment: string; project: string }> {
    const p = await this.project(team);
    const e = await this.api<{ id: string }>(`/api/projects/${p.id}/environments`, { name, driver, secret_names: secrets });
    return { environment: e.id, project: p.id };
  }

  /** As an owner, make an environment with a driver's settings (M220,
   * M233) and no secrets. */
  async environmentWith(name: string, driver: string, settings: Record<string, string>, team?: string): Promise<{ environment: string; project: string }> {
    const p = await this.project(team);
    const e = await this.api<{ id: string }>(`/api/projects/${p.id}/environments`, { name, driver, secret_names: [], settings });
    return { environment: e.id, project: p.id };
  }

  /** Mint an agent grant token (M230) over the account's project, or
   * TEAM's, with `powers`: the token, shown once. */
  async agentToken(name: string, powers: string[], team?: string): Promise<{ token: string; id: string; project: string }> {
    const p = await this.project(team);
    const m = await this.api<{ token: string; agent_token: { id: string } }>("/api/agent-tokens", { name, projects: [p.id], powers });
    return { token: m.token, id: m.agent_token.id, project: p.id };
  }

  /** As an owner, set one of an environment's secrets (M219). */
  async environmentSecret(env: string, name: string, value: string): Promise<{ set: boolean }> {
    return this.api<{ set: boolean }>(`/api/environments/${env}/secrets/${name}`, { value });
  }

  /** Plan a deploy of commit REF from WHERE to an environment, and ask
   * for its approval: its id and operation hash. */
  async planDeploy(env: string, ref: string, where: string, team?: string): Promise<{ deploy: string; op: string; state: string }> {
    const p = await this.project(team);
    // The platform makes the plan: its driver's (M219).
    const d = await this.api<{ id: string }>(`/api/projects/${p.id}/deploys`, { environment: env, source: { kind: "commit", ref, where } });
    const r = await this.api<{ id: string; state: string; operation: { hash: string } }>(`/api/deploys/${d.id}/request`, {});
    return { deploy: r.id, op: r.operation.hash, state: r.state };
  }

  /** As an owner, approve a deploy once: this device signs its operation,
   * and the passkey confirms the approval's bytes, as a phone does. */
  async approveDeploy(id: string, lifeMs = DEFAULT_LIFE_MS): Promise<{ deploy: string; state: string }> {
    const d = await this.api<{ operation: { hash: string } }>(`/api/deploys/${id}`);
    const unsigned = { deploy: id, op: d.operation.hash, account: this.account, decision: "once" as const, expires: Date.now() + lifeMs, by: this.keys.id };
    const a = await signApproval(unsigned, this.keys.sign.privateKey);
    const challenge = b64url(await sha256(approvalBody(unsigned)));
    const passkey = await this.assert(challenge);
    const r = await this.api<{ id: string; state: string }>(`/api/deploys/${id}/approve`, { op: a.op, expires: a.expires, by: a.by, sig: a.sig, passkey });
    return { deploy: r.id, state: r.state };
  }

  /** A deploy as its members read it. */
  async deploy(id: string): Promise<{ state: string; run: string | null; events: { kind: string }[] }> {
    return this.api(`/api/deploys/${id}`);
  }

  /** As an owner, let control enroll hosted machines into the project. */
  async delegate(team?: string): Promise<{ project: string; owner: string; delegation: string }> {
    const p = await this.project(team);
    const { key } = await this.api<{ key: string }>("/api/hosted/delegations");
    const d = await signDelegation(p.id, p.owner, key, this.keys);
    const r = await this.api<{ id: string }>("/api/hosted/delegations", { delegation: d });
    return { project: p.id, owner: p.owner, delegation: r.id };
  }

  /** As an owner, take back the project's live delegations. How many. */
  async undelegate(team?: string): Promise<number> {
    const p = await this.project(team);
    const { delegations } = await this.api<{ delegations: { delegation: Delegation; revoke: DelegationRevoke | null }[] }>("/api/hosted/delegations");
    let n = 0;
    for (const x of delegations) {
      if (x.delegation.project !== p.id || x.revoke) continue;
      await this.api("/api/hosted/delegations/revoke", { revoke: await signRevoke(x.delegation, this.keys) });
      n++;
    }
    return n;
  }

  /** As an owner, sign the project's standing grant for runs (M211) under
   * its live delegation, as control's page does. */
  async signRunGrant(team?: string): Promise<{ project: string; grant: string }> {
    const p = await this.project(team);
    const { key, delegations } = await this.api<{ key: string; delegations: { delegation: Delegation; revoke: DelegationRevoke | null }[] }>("/api/hosted/delegations");
    const live = delegations.find((x) => x.delegation.project === p.id && x.delegation.key === key && !x.revoke);
    if (!live) throw new Error(`project ${p.id} has no live delegation`);
    const grant = await signGrant(live.delegation, this.keys);
    await this.api(`/api/projects/${p.id}/runs/grant`, { grant });
    return { project: p.id, grant: grant.id };
  }

  /** As an owner, revoke the project's standing grant; `stop`: the runs
   * going stop too. */
  async revokeRunGrant(team?: string, stop = false): Promise<{ project: string; grant: string; told: number }> {
    const p = await this.project(team);
    const v = await this.api<{ grant: { grant: ProjectGrant } | null }>(`/api/projects/${p.id}/runs/grant`);
    if (!v.grant) throw new Error(`project ${p.id} has no standing grant`);
    const revoke = await signGrantRevoke(p.id, v.grant.grant.delegation, stop ? "stop" : "finish", this.keys);
    const r = await this.api<{ told: number }>(`/api/projects/${p.id}/runs/grant/revoke`, { revoke });
    return { project: p.id, grant: v.grant.grant.id, told: r.told };
  }

  /** Start a run of `argv` on a hosted workspace of the account's project,
   * or TEAM's (M214's start by hand; under the live grant, M211): its id. */
  async startRun(workspace: string, argv: string[], team?: string, directory?: string): Promise<{ run: string; state: string }> {
    const p = await this.project(team);
    const r = await this.api<{ id: string; state: string }>(`/api/projects/${p.id}/runs`, { workspace, argv, directory });
    return { run: r.id, state: r.state };
  }

  /** Start an agent run (M212): Claude Code with `instruction`, headless. */
  async startAgentRun(workspace: string, instruction: string, team?: string): Promise<{ run: string; state: string }> {
    const p = await this.project(team);
    const r = await this.api<{ id: string; state: string }>(`/api/projects/${p.id}/runs`, { workspace, instruction });
    return { run: r.id, state: r.state };
  }

  /** A hosted machine as the directory lists it here, with its proof. */
  async hostedEntry(id: string): Promise<(Listed & { team: string | null; hosted: HostedProof }) | undefined> {
    const all = (await this.api<{ daemons: (Listed & { team: string | null; hosted?: HostedProof })[] }>("/api/directory")).daemons;
    const d = all.find((x) => x.id === id || x.name === id);
    return d?.hosted ? (d as Listed & { team: string | null; hosted: HostedProof }) : undefined;
  }

  /** Whether this device accepts a hosted machine (its proof as the
   * directory lists it, here or to someone else), as the page does:
   * against its team's roster or its own account's chain. */
  async acceptHosted(proof: HostedProof): Promise<Awaited<ReturnType<typeof acceptHosted>>> {
    const owner = proof.delegation.owner;
    if (owner.startsWith("team:")) {
      const t = await this.team(owner.slice(5)).catch(() => null);
      if (!t) return { ok: false, why: "not_in_project" };
      return acceptHosted(this.account, proof, { team: t.roster, certs: t.certs });
    }
    const all = await this.api<{ certs: Cert[]; revocations?: Revocation[] }>("/api/devices");
    return acceptHosted(this.account, proof, { account: { account: this.account, root: this.root }, certs: all.certs, revocations: all.revocations ?? [] });
  }

  /** Whether control's relay routes this device's person to `id`: the
   * socket opens (control spliced it), or it doesn't. */
  async routed(id: string, timeoutMs = 5000): Promise<boolean> {
    const url = `${this.reach(this.control).replace(/^http/, "ws")}/api/relay/c/${id}`;
    const Ws = globalThis.WebSocket as unknown as new (u: string, o: unknown) => WebSocket;
    const ws = new Ws(url, { headers: { cookie: this.cookie, origin: this.control } });
    try {
      return await new Promise<boolean>((res) => {
        const t = setTimeout(() => res(false), timeoutMs);
        ws.onopen = () => (clearTimeout(t), res(true));
        ws.onerror = () => (clearTimeout(t), res(false));
      });
    } finally {
      ws.close();
    }
  }

  /** M186: what a browser block shows of a port on daemon `id`, as a
   * block's frame on control's page loads it. With `port`, this device opens
   * a port block there over its own channel (learning the daemon's clock and
   * the block's site key); with `block`, it uses one already open (an
   * outsider's try: the daemon's Noise key from `noise`, the expiry by this
   * clock). Then it signs a grant for a one-off key, opens the block's
   * channel through control's relay on the block's own origin
   * (`origin(siteKey)`, reached as `reach` says), and fetches `path`.
   * Returns the block, the status and body, or the daemon's refusal. */
  async preview(
    id: string,
    o: { port?: number; block?: { id: string; siteKey: string }; noise?: string; origin: (siteKey: string) => string; path?: string; timeoutMs?: number },
  ): Promise<{ block: string; siteKey: string; status?: number; body?: string; refused?: string }> {
    const timeoutMs = o.timeoutMs ?? 20_000;
    let block = o.block;
    let noise = o.noise;
    let now = Date.now();
    if (o.port !== undefined) {
      const sock = await this.connect(id);
      try {
        const texts: string[] = [];
        sock.onText = (t) => texts.push(t);
        sock.start();
        const until = Date.now() + timeoutMs;
        while (!texts.some((t) => t.includes('"hello"'))) {
          if (Date.now() > until) throw new Error("no hello from the daemon");
          await sleep(100);
        }
        if (!sock.clock) throw new Error("the daemon sent no clock");
        const before = texts.length;
        const r = await sock.request("POST", "/api/blocks", { type: "browser", config: { port: o.port, path: "/" }, split: null }, timeoutMs);
        if (!r.ok) throw new Error(`opening a port block: ${r.status} ${await r.text()}`);
        const pane = String(((await r.json()) as { block: number }).block);
        // The block's state, with its site key, comes on the channel.
        const re = /"site_key":"([0-9a-f]{16,61})"/;
        while (!block) {
          const hit = texts.slice(before).map((t) => re.exec(t)).findLast(Boolean);
          if (hit) block = { id: pane, siteKey: hit[1] };
          else if (Date.now() > until) throw new Error(`no site key for block ${pane}`);
          else await sleep(100);
        }
        now = daemonNow(sock.clock);
        noise = (await this.hostedEntry(id))?.hosted.cert.noise ?? (await this.trusted()).get(id)?.noise;
      } finally {
        sock.close();
      }
    }
    if (!block || !noise) throw new Error("a port or a block, and the daemon's Noise key");
    const origin = o.origin(block.siteKey);
    blockHeaders.origin = origin;
    const { BlockChannel, Refused } = await import("../src/shell/block/channel.ts");
    const creds = await mint({ keys: this.keys, daemon: { id, noise }, block: block.id, now, route: "relay" });
    const url = `${origin.replace(/^http/, "ws")}/.arugula/relay?d=${encodeURIComponent(id)}`;
    try {
      const ch = await BlockChannel.connect(url, creds, timeoutMs);
      try {
        const res = await ch.fetch("GET", o.path ?? "/", [["accept", "text/html"]], new Uint8Array());
        const body = await new Response(res.body).text();
        return { block: block.id, siteKey: block.siteKey, status: res.head.status, body };
      } finally {
        ch.close();
      }
    } catch (e) {
      if (e instanceof Refused) return { block: block.id, siteKey: block.siteKey, refused: e.code };
      throw e;
    }
  }

  async directory(): Promise<Listed[]> {
    return (await this.api<{ daemons: Listed[] }>("/api/directory")).daemons;
  }

  /** Wait until the directory lists a daemon (by name or id) as online. */
  async waitOnline(daemon: string, timeoutMs = 30_000): Promise<Listed> {
    const until = Date.now() + timeoutMs;
    let seen: Listed | undefined;
    for (;;) {
      seen = (await this.directory()).find((d) => d.id === daemon || d.name === daemon);
      if (seen?.online) return seen;
      if (Date.now() > until) throw new Error(`${daemon} isn't online after ${timeoutMs / 1000}s: ${JSON.stringify(seen ?? "not listed")}`);
      await sleep(250);
    }
  }

  /** An end-to-end socket to a daemon the account trusts: through
   * control's relay, or `direct` (its own `/e2e` URL). Its Noise key comes
   * from a certificate that chains to this device's root, never from the
   * directory alone. */
  async connect(daemonId: string, direct?: string, timeoutMs = 5000, how: { mux?: boolean; wake?: boolean; onWake?: (w: WakeSaid) => void } = {}): Promise<E2ESocket> {
    const cert = (await this.trusted()).get(daemonId);
    // A hosted machine (M178, M180): its Noise key from its hosted
    // certificate, once this device accepts it for its project, as the page does.
    const hosted = cert ? undefined : await this.hostedEntry(daemonId);
    const accepted = hosted && (await this.acceptHosted(hosted.hosted)).ok;
    const noise = cert?.kind === "daemon" ? cert.noise : accepted ? hosted.hosted.cert.noise : undefined;
    if (!noise) throw new Error(`${daemonId} isn't a daemon this account trusts`);
    const ws = this.reach(this.control).replace(/^http/, "ws");
    const url = direct ?? `${ws}/api/relay/c/${daemonId}`;
    const headers = direct ? {} : { cookie: this.cookie, origin: this.control };
    const orig = globalThis.WebSocket;
    // Node's WebSocket (undici) takes headers as a second argument; the
    // socket is made synchronously inside connect, so swapping the global
    // for the call is enough.
    globalThis.WebSocket = class extends orig {
      constructor(u: string | URL) {
        super(u, { headers } as unknown as string[]);
      }
    } as typeof WebSocket;
    // M182: as the page does, a channel in the shared socket, which can
    // ask to wake a hosted machine that's asleep.
    const mux = how.mux && !direct ? `${ws}/api/relay/m` : undefined;
    try {
      return await E2ESocket.connect([{ url, timeoutMs, mux, wake: how.wake, onWake: how.onWake }], { id: daemonId, noise }, this.keys);
    } finally {
      globalThis.WebSocket = orig;
    }
  }

  /** Open a machine as the page does (M182): through the shared socket,
   * asking to wake it if `wake`, then its hello and its first pane's
   * screen. How long each took from the open (ms), and what control said
   * of a hosted machine's box. */
  async open(daemonId: string, opts: { wake?: boolean; timeoutMs?: number } = {}): Promise<{ hello: number; ready: number; said: string[] }> {
    const said: string[] = [];
    const t0 = performance.now();
    const sock = await this.connect(daemonId, undefined, opts.timeoutMs ?? 15_000, { mux: true, wake: opts.wake ?? true, onWake: (w) => said.push(w.state) });
    try {
      const texts: string[] = [];
      let screen = false;
      sock.onText = (t) => texts.push(t);
      sock.onBinary = (b) => {
        if (b[0] === 1 || b[0] === 2) screen = true;
      };
      sock.start();
      const until = Date.now() + (opts.timeoutMs ?? 15_000);
      const wait = async (what: string, ok: () => boolean) => {
        while (!ok()) {
          if (Date.now() > until) throw new Error(`timed out waiting for ${what}`);
          await sleep(5);
        }
      };
      await wait("the daemon's hello", () => texts.some((t) => t.includes('"hello"')));
      const hello = performance.now() - t0;
      const pane: number | undefined = JSON.parse(texts.find((t) => t.includes('"hello"'))!).state?.panes?.[0]?.id;
      if (pane !== undefined) {
        sock.sendText(JSON.stringify({ type: "attach", panes: [{ pane, offset: null }] }));
        await wait("its first pane's screen", () => screen);
      }
      return { hello: Math.round(hello), ready: Math.round(performance.now() - t0), said };
    } finally {
      sock.close();
    }
  }

  /** The daemon's panes, from its hello over the relay, without touching
   * them (typing would make this device their driver). */
  async panes(daemonId: string, timeoutMs = 15_000): Promise<number[]> {
    const sock = await this.connect(daemonId);
    try {
      let hello: { state?: { panes?: { id: number }[] } } | undefined;
      sock.onText = (t) => {
        if (!hello && t.includes('"hello"')) hello = JSON.parse(t);
      };
      sock.start();
      const until = Date.now() + timeoutMs;
      while (!hello) {
        if (Date.now() > until) throw new Error("no hello from the daemon");
        await sleep(100);
      }
      return (hello.state?.panes ?? []).map((p) => p.id);
    } finally {
      sock.close();
    }
  }

  /** A pane's text (`GET /api/panes/<pane>/capture`), over the relay,
   * without typing into it. */
  async capture(daemonId: string, pane: number, timeoutMs = 15_000): Promise<string> {
    const sock = await this.connect(daemonId);
    try {
      let hello = false;
      sock.onText = (t) => {
        if (t.includes('"hello"')) hello = true;
      };
      sock.start();
      const until = Date.now() + timeoutMs;
      while (!hello) {
        if (Date.now() > until) throw new Error("no hello from the daemon");
        await sleep(100);
      }
      const r = await sock.request("GET", `/api/panes/${pane}/capture?format=text`, undefined, timeoutMs);
      if (!r.ok) throw new Error(`capture of pane ${pane}: ${r.status}`);
      return await r.text();
    } finally {
      sock.close();
    }
  }

  /** Type `echo <marker>-$((6*7))` into the daemon's first pane and wait
   * for `<marker>-42` to come back: a pane reached and working, both ways.
   * Returns what the pane printed. Through the relay unless `direct`. */
  async roundTrip(daemonId: string, marker: string, opts: { direct?: string; timeoutMs?: number } = {}): Promise<string> {
    const sock = await this.connect(daemonId, opts.direct);
    try {
      const texts: string[] = [];
      let out = "";
      sock.onText = (t) => texts.push(t);
      sock.onBinary = (b) => {
        if (b[0] === 1 || b[0] === 2) out += new TextDecoder().decode(b.subarray(13));
      };
      sock.start();
      const until = Date.now() + (opts.timeoutMs ?? 15_000);
      const wait = async (what: string, ok: () => boolean) => {
        while (!ok()) {
          if (Date.now() > until) throw new Error(`timed out waiting for ${what}; the pane showed ${JSON.stringify(out.slice(-300))}`);
          await sleep(100);
        }
      };
      await wait("the daemon's hello", () => texts.some((t) => t.includes('"hello"')));
      const hello = JSON.parse(texts.find((t) => t.includes('"hello"'))!);
      const pane: number | undefined = hello.state?.panes?.[0]?.id;
      if (pane === undefined) throw new Error("the daemon has no panes");
      sock.sendText(JSON.stringify({ type: "attach", panes: [{ pane, offset: null }] }));
      const input = new TextEncoder().encode(`echo ${marker}-$((6*7))\n`);
      const frame = new Uint8Array(13 + input.length);
      frame[0] = 3;
      new DataView(frame.buffer).setUint32(1, pane);
      frame.set(input, 13);
      sock.sendBinary(frame);
      await wait(`${marker}-42`, () => out.includes(`${marker}-42`));
      return out;
    } finally {
      sock.close();
    }
  }

  /** Watch pane `pane` of a daemon over control's relay, end to end, as
   * a phone does, until its output shows `text`: what it showed. */
  async watch(daemonId: string, pane: number, text: string, timeoutMs = 15_000): Promise<string> {
    const sock = await this.connect(daemonId);
    try {
      const texts: string[] = [];
      let out = "";
      sock.onText = (t) => texts.push(t);
      sock.onBinary = (b) => {
        if ((b[0] === 1 || b[0] === 2) && new DataView(b.buffer, b.byteOffset).getUint32(1) === pane) {
          out += new TextDecoder().decode(b.subarray(13));
        }
      };
      sock.start();
      const until = Date.now() + timeoutMs;
      while (!texts.some((t) => t.includes('"hello"'))) {
        if (Date.now() > until) throw new Error("no hello from the daemon");
        await sleep(100);
      }
      sock.sendText(JSON.stringify({ type: "attach", panes: [{ pane, offset: null }] }));
      while (!out.includes(text)) {
        if (Date.now() > until) throw new Error(`pane ${pane} didn't show ${text}; it showed ${JSON.stringify(out.slice(-300))}`);
        await sleep(100);
      }
      return out;
    } finally {
      sock.close();
    }
  }

  /** Keep this device in a file (keys, session, pinned root), for a test
   * that runs in steps. */
  async save(file: string): Promise<void> {
    const jwk = (k: CryptoKey) => subtle.exportKey("jwk", k);
    const s: Saved = {
      control: this.control,
      via: this.via,
      cookie: this.cookie,
      account: this.account,
      login: this.login,
      root: this.root,
      name: this.name,
      approved: this.approved,
      keys: {
        id: this.keys.id,
        noisePub: this.keys.noisePub,
        signPub: this.keys.signPub,
        noise: await jwk(this.keys.noise.privateKey),
        sign: await jwk(this.keys.sign.privateKey),
      },
      passkey: this.passkey && { id: this.passkey.id, rpId: this.passkey.rpId, pub: this.passkey.pub, key: await jwk(this.passkey.key) },
    };
    writeFileSync(file, JSON.stringify(s, null, 2), { mode: 0o600 });
  }

  static async load(file: string): Promise<Device> {
    const s = JSON.parse(readFileSync(file, "utf8")) as Saved;
    const pair = async (alg: string, priv: JsonWebKey, pub: string, use: KeyUsage[], pubUse: KeyUsage[]): Promise<CryptoKeyPair> => ({
      privateKey: await subtle.importKey("jwk", priv, { name: alg }, true, use),
      publicKey: await subtle.importKey("raw", unhex(pub), { name: alg }, true, pubUse),
    });
    const keys: DeviceKeys = {
      id: s.keys.id,
      noisePub: s.keys.noisePub,
      signPub: s.keys.signPub,
      noise: await pair("X25519", s.keys.noise, s.keys.noisePub, ["deriveBits"], []),
      sign: await pair("Ed25519", s.keys.sign, s.keys.signPub, ["sign"], ["verify"]),
    };
    const d = new Device({ control: s.control, via: s.via }, keys);
    Object.assign(d, { cookie: s.cookie, account: s.account, login: s.login, root: s.root, name: s.name, approved: s.approved });
    if (s.passkey) {
      const key = await subtle.importKey("jwk", s.passkey.key, { name: "Ed25519" }, true, ["sign"]);
      d.passkey = { id: s.passkey.id, rpId: s.passkey.rpId, pub: s.passkey.pub, key };
    }
    return d;
  }
}
