// The standing project grant (M211): the browser's copy of
// `arugula_e2e::project_grant`, byte for byte (fixed line formats, not
// JSON). An owner says, once per project, that the platform may start and
// instruct runs on the project's hosted workspaces, under the project's
// delegation; any owner revokes it. The platform keeps and relays both;
// hosted machines check them.
//
// Since DK6 (#387, aru-126) a grant is v2 and a revoke v3: no device signs
// either, and the signature is a user-verified passkey assertion whose
// challenge is SHA-256 of these bytes (`passkeyChallenge`):
//
//   arugula project grant v2            arugula project grant revoke v3
//   id <32 hex>                         project <prj id>
//   project <prj id>                    delegation <id>
//   owner team:<id>|account:<id>        running <finish|stop>
//   delegation <id>                     at <ms>
//   resource hosted                     account <the revoking owner>
//   to platform
//   caps runs.instruct
//   caps runs.start
//   made <ms>
//   expires <ms>
//   account <the granting owner>
//
// v1 grants (`by <device id>` last, an expiry or `never`) and the device's
// revoke (`arugula project grant revoke v2`, `by` last) were signed with
// the browser's device key. The page makes neither now; it reads them, as
// the platform still holds v1 grants until they expire.

import { delegationId, type Delegation } from "./hosted.ts";
import { hex } from "./cert.ts";

/** What the grant gives, sorted: as Rust's `CAPS`. */
export const RUN_CAPS = ["runs.instruct", "runs.start"] as const;

/** A passkey's assertion as the platform takes it: each base64url. */
export interface PasskeyAssertion {
  id: string;
  clientDataJSON: string;
  authenticatorData: string;
  signature: string;
}

export interface ProjectGrant {
  id: string;
  project: string;
  /** `team:<id>` or `account:<id>`. */
  owner: string;
  /** The project's delegation's id. */
  delegation: string;
  caps: string[];
  made: number;
  /** When it stops counting (ms); null: never (v1 only). */
  expires: number | null;
  /** v1: the signing device. Empty in v2. */
  by: string;
  /** v1: its signature, hex. Empty in v2. */
  sig: string;
  /** v2: the granting owner's account. */
  account?: string;
  /** v2: the passkey's assertion over its bytes. */
  passkey?: PasskeyAssertion;
  /** 2 (DK6) or, absent, 1. */
  v?: number;
}

/** Every grant for `project` under `delegation` made at or before `at`,
 * v1 or v2: it names no grant, so whoever relays it can't choose one. */
export interface GrantRevoke {
  project: string;
  delegation: string;
  /** What happens to runs going: they finish, or they're stopped too. */
  running: "finish" | "stop";
  at: number;
  /** A device's revoke: the signing device. Empty in v3. */
  by: string;
  /** A device's revoke: its signature, hex. Empty in v3. */
  sig: string;
  /** v3: the revoking owner's account. */
  account?: string;
  /** v3: the passkey's assertion over its bytes. */
  passkey?: PasskeyAssertion;
  /** 3 (DK6) or, absent, 2 (a device's). */
  v?: number;
}

export const grantBody = (g: ProjectGrant) => {
  const v = g.v ?? 1;
  const signer = v === 1 ? `by ${g.by}` : `account ${g.account ?? ""}`;
  return (
    `arugula project grant v${v}\nid ${g.id}\nproject ${g.project}\nowner ${g.owner}\ndelegation ${g.delegation}\nresource hosted\nto platform\n` +
    g.caps.map((c) => `caps ${c}\n`).join("") +
    `made ${g.made}\nexpires ${g.expires ?? "never"}\n${signer}\n`
  );
};

export const grantRevokeBody = (r: GrantRevoke) => {
  const v = r.v ?? 2;
  const signer = v === 2 ? `by ${r.by}` : `account ${r.account ?? ""}`;
  return `arugula project grant revoke v${v}\nproject ${r.project}\ndelegation ${r.delegation}\nrunning ${r.running}\nat ${r.at}\n${signer}\n`;
};

/** The WebAuthn challenge a passkey signs over `body`: its SHA-256, as
 * Rust's `passkey_challenge` (which base64url-encodes the same bytes). */
export async function passkeyChallenge(body: string): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body)));
}

/** A new grant's id: 32 hex digits from 16 random bytes. */
const newId = () => hex(globalThis.crypto.getRandomValues(new Uint8Array(16)));

/** How long a grant made here lasts unless the owner picks (S37 q7a). */
export const GRANT_DAYS = 90;
const DAY_MS = 24 * 60 * 60 * 1000;

/** `account`'s v2 grant of runs on `d`'s project until `expires`, not yet
 * confirmed: its passkey assertion over `grantBody` is its signature. */
export async function newGrant(d: Delegation, account: string, expires: number, id = newId(), made = Date.now()): Promise<ProjectGrant> {
  return {
    id,
    project: d.project,
    owner: d.owner,
    delegation: await delegationId(d),
    caps: [...RUN_CAPS],
    made,
    expires,
    by: "",
    sig: "",
    account,
    v: 2,
  };
}

/** When a grant lasting `days` from now expires. */
export const expiresIn = (days: number, from = Date.now()) => from + days * DAY_MS;

/** `account`'s v3 revoke of every grant for `project` under `delegation`
 * made by `at`, not yet confirmed; `running: "stop"` stops the runs going
 * too. */
export function newGrantRevoke(project: string, delegation: string, running: GrantRevoke["running"], account: string, at = Date.now()): GrantRevoke {
  return { project, delegation, running, at, by: "", sig: "", account, v: 3 };
}
