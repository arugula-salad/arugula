// A team's roster and its invite links, as both control's page and the
// app's Settings change them (Arugula https://app.arugula.io/issues/iss_e40bd4fd2e5488c4ce07e160bd67752b):
// the next version of a roster, and an invite link with its one-time key.
// Neither signs: control's page signs with its device key where it has one
// (shell/control.ts, shell/e2e/team.ts), and everything else goes on
// control's word (aru-141, Arugula #1005), unsigned. The shapes are
// `arugula_e2e::team`'s.

export type TeamRole = "owner" | "editor" | "viewer";

export interface Member {
  account: string;
  root: string;
  role: TeamRole;
  name: string;
}

export interface Roster {
  v: number;
  team: string;
  name: string;
  version: number;
  at: number;
  members: Member[];
  /** Invites already redeemed (v2), so each works once. */
  spent?: Spent[];
  /** The presigned invite this version redeems (v2). */
  redeem?: Redeem;
  by: string;
  sig: string;
}

/** An invite an owner's device signed ahead of time (`team::Invite`):
 * whoever holds the one-time key's private half adds themselves, once. */
export interface Invite {
  team: string;
  role: TeamRole;
  expires: number;
  key: string;
  by: string;
  sig: string;
}

/** An *Ask me first* invite (`team::AskInvite`, #762): signed by an
 * owner's device like a presigned one, with its own one-time key, but an
 * owner still says yes to each person who uses it. */
export interface AskInvite {
  team: string;
  role: TeamRole;
  made: number;
  expires: number;
  key: string;
  by: string;
  sig: string;
}

export interface Spent {
  key: string;
  expires: number;
}

export interface Redeem {
  invite: Invite;
  proof: string;
}

export interface TeamPin {
  team: string;
  founder: string;
  founder_root: string;
}

/** A one-click invite link control still holds (#134). */
export interface PresignedInvite {
  key: string;
  role: TeamRole;
  expires: number;
  by: string;
  by_name: string;
}

/** How long a presigned invite lasts unless the owner says otherwise. */
export const PRESIGNED_TTL_MS = 24 * 3600_000;

/** How long an Ask me first link lasts. */
export const ASK_TTL_MS = 7 * 24 * 3600_000;

/** A name in a roster: no spaces or control characters. */
export const word = (s: string) => s.replace(/[\s\u0000-\u001f\u007f-\u009f]+/g, "-").slice(0, 120) || "someone";

/** Whether `r` changes who the team's owners are since `prev`
 * (`Roster::changes_owners`): someone made an owner or no longer one, or an
 * owner's entry moved to another root. Control takes such a version only
 * with an owner's passkey over it (DK7). */
export function changesOwners(r: Pick<Roster, "members">, prev: Pick<Roster, "members">): boolean {
  const owners = (x: Pick<Roster, "members">) =>
    x.members
      .filter((m) => m.role === "owner")
      .map((m) => `${m.account} ${m.root}`)
      .sort()
      .join("\n");
  return owners(r) !== owners(prev);
}

/** The version after `prev`, its members as `change` makes them, not yet
 * signed. Used presigned invites stay listed until they'd have expired
 * anyway, so none works twice. */
export function nextRoster(prev: Roster, change: (members: Member[]) => Member[], name?: string): Omit<Roster, "by" | "sig"> {
  const now = Date.now();
  const spent = (prev.spent ?? []).filter((x) => x.expires > now);
  return {
    v: spent.length ? 2 : 1,
    team: prev.team,
    name: name ?? prev.name,
    version: prev.version + 1,
    at: now,
    members: change(prev.members.map((m) => ({ ...m }))),
    ...(spent.length ? { spent } : {}),
  };
}

/** A new X25519 or Ed25519 key pair. WebKit on Linux (libgcrypt) fails
 * about one generateKey in 200 with an OperationError (#524: 10 to 22 in
 * 3000 for each kind, in Playwright's WebKit); a fresh try succeeds, so
 * try again. */
export async function newKeyPair(name: "X25519" | "Ed25519", extractable: boolean, usages: KeyUsage[]): Promise<CryptoKeyPair> {
  for (let attempt = 1; ; attempt++) {
    try {
      return (await crypto.subtle.generateKey({ name }, extractable, usages)) as CryptoKeyPair;
    } catch (e) {
      if (attempt >= 8 || (e as Error).name !== "OperationError") throw e;
    }
  }
}

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

// An Ed25519 private key as PKCS#8 is this prefix and the 32-byte seed, so
// the link carries only the seed.
export const PKCS8_ED25519 = "302e020100300506032b657004220420";

/** A one-time invite key: its seed (for the link's fragment only) and
 * public half (the invite's `key`). */
export async function newInviteKey(): Promise<{ seed: string; key: string }> {
  const k = await newKeyPair("Ed25519", true, ["sign", "verify"]);
  const pkcs8 = hex(new Uint8Array(await crypto.subtle.exportKey("pkcs8", k.privateKey)));
  return { seed: pkcs8.slice(PKCS8_ED25519.length), key: hex(new Uint8Array(await crypto.subtle.exportKey("raw", k.publicKey))) };
}

/** An invite link that was made: whether it asks an owner first, and why
 * when it does though it wasn't asked to. */
export interface MadeInvite {
  link: string;
  asks: boolean;
  why?: string;
}

/** Make an invite link for `team`: presigned unless `askFirst` (or for an
 * owner), so whoever opens it is in as soon as they accept; else one that
 * asks an owner first. Either kind's one-time key seed goes only into the
 * link's fragment, and the link opens on `origin`, control's page, which
 * signs the person in and redeems it. `sign` is the device's, where it
 * signs for the team; without it the invite is control's word (aru-141),
 * which control takes from an owner signed in. `post` sends a JSON body
 * and throws an error with the answer's `status`: control makes a
 * presigned one only once every machine checking the team's rosters
 * understands it, and says 409 until then. */
export async function makeInvite(o: {
  team: string;
  role: TeamRole;
  askFirst: boolean;
  origin: string;
  post: (path: string, body: unknown) => Promise<unknown>;
  sign?: { invite: (i: Omit<Invite, "by" | "sig">) => Promise<Invite>; ask: (i: Omit<AskInvite, "by" | "sig">) => Promise<AskInvite> };
}): Promise<MadeInvite> {
  const { team, role } = o;
  // #762: an Ask me first link carries a one-time key too, so a request
  // to join proves it came from the link.
  const old = async (why?: string): Promise<MadeInvite> => {
    const { seed, key } = await newInviteKey();
    const made = Date.now();
    const draft = { team, role, made, expires: made + ASK_TTL_MS, key };
    const ask = o.sign ? await o.sign.ask(draft) : { ...draft, by: "", sig: "" };
    await o.post(`/api/teams/${team}/invites`, { role, ask });
    return { link: `${o.origin}/#askinvite=${team}.${seed}`, asks: true, why };
  };
  if (o.askFirst || role === "owner") return old();
  const { seed, key } = await newInviteKey();
  const draft = { team, role, expires: Date.now() + PRESIGNED_TTL_MS, key };
  const presigned = o.sign ? await o.sign.invite(draft) : { ...draft, by: "", sig: "" };
  try {
    await o.post(`/api/teams/${team}/invites`, { role, presigned });
  } catch (e) {
    if ((e as { status?: number }).status === 409) return old((e as Error).message);
    throw e;
  }
  return { link: `${o.origin}/#pinvite=${team}.${seed}`, asks: false };
}

/** How long a link has left, roughly. */
export function expiresIn(at: number): string {
  const min = Math.max(1, Math.round((at - Date.now()) / 60_000));
  return min < 90 ? `expires in ${min} min` : `expires in ${Math.round(min / 60)} h`;
}
