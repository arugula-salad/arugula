// The standing project grant (M211): the browser's copy of
// `arugula_e2e::project_grant`, byte for byte (fixed line formats, not
// JSON). An owner's device signs, once per project, that the platform may
// start and instruct runs on the project's hosted workspaces, under the
// project's delegation; any owner's device revokes it. The platform keeps
// and relays both; hosted machines check them.

import { hex } from "./cert.ts";
import { delegationId, type Delegation } from "./hosted.ts";
import { signText, type DeviceKeys } from "./keys.ts";

/** What the grant gives, sorted: as Rust's `CAPS`. */
export const RUN_CAPS = ["runs.instruct", "runs.start"] as const;

export interface ProjectGrant {
  id: string;
  project: string;
  /** `team:<id>` or `account:<id>`. */
  owner: string;
  /** The project's delegation's id. */
  delegation: string;
  caps: string[];
  made: number;
  /** When it stops counting (ms); null: never. */
  expires: number | null;
  by: string;
  sig: string;
}

export interface GrantRevoke {
  project: string;
  grant: string;
  /** What happens to runs going: they finish, or they're stopped too. */
  running: "finish" | "stop";
  at: number;
  by: string;
  sig: string;
}

export const grantBody = (g: ProjectGrant) =>
  `arugula project grant v1\nid ${g.id}\nproject ${g.project}\nowner ${g.owner}\ndelegation ${g.delegation}\nresource hosted\nto platform\n` +
  g.caps.map((c) => `caps ${c}\n`).join("") +
  `made ${g.made}\nexpires ${g.expires ?? "never"}\nby ${g.by}\n`;

export const grantRevokeBody = (r: GrantRevoke) =>
  `arugula project grant revoke v1\nproject ${r.project}\ngrant ${r.grant}\nrunning ${r.running}\nat ${r.at}\nby ${r.by}\n`;

/** A new grant's id: 32 hex digits from 16 random bytes. */
const newId = () => hex(globalThis.crypto.getRandomValues(new Uint8Array(16)));

/** An owner's device grants runs on `d`'s project, until it's revoked (or
 * `expires`, if given). */
export async function signGrant(d: Delegation, keys: DeviceKeys, expires: number | null = null, id = newId(), made = Date.now()): Promise<ProjectGrant> {
  const g: ProjectGrant = {
    id,
    project: d.project,
    owner: d.owner,
    delegation: await delegationId(d),
    caps: [...RUN_CAPS],
    made,
    expires,
    by: keys.id,
    sig: "",
  };
  return { ...g, sig: await signText(keys, grantBody(g)) };
}

/** An owner's device takes a grant back; `running: "stop"` stops the runs
 * going too. */
export async function signGrantRevoke(g: ProjectGrant, running: GrantRevoke["running"], keys: DeviceKeys, at = Date.now()): Promise<GrantRevoke> {
  const r: GrantRevoke = { project: g.project, grant: g.id, running, at, by: keys.id, sig: "" };
  return { ...r, sig: await signText(keys, grantRevokeBody(r)) };
}
