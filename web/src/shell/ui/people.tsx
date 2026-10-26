// Other people in a session (M13): who's here and where they look, who
// drives each pane, handing control over, following someone, and sharing a
// session.

import { OPS, opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { InviteRequest, Invited, PaneId, Presence, Role, SessionId } from "../../shared/proto";
import { roleLabel } from "./roles";
import type { MenuItem } from "../../shared/ui/menu";
import { keyChanged, keyChangeSeen, personPin, pinPerson, verifyPerson, type ControlSession } from "../control";
import { readVerifyCode, verifyWords } from "../e2e/words.ts";
import { CopyText } from "../../shared/ui/copy";
import { countView } from "../../shared/telemetry";
import { canScan, Scanner } from "./scan";
import { useEscape } from "./keys";
import { colorOf } from "../../shared/chat/avatar";

/** Control mode (M19): people are accounts there, and links go through it. */
let control: ControlSession | null = null;
export const getControlSession = () => control;
export function setControlSession(s: ControlSession | null) {
  control = s;
}

/** A steady colour per person (chat's avatars share it). */
export { colorOf };

function initials(name: string): string {
  const base = name.split("@")[0];
  const parts = base.split(/[._\s-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? "?") + (parts[1]?.[0] ?? "")).toUpperCase();
}

export function Avatar({ p, onClick, title }: { p: Pick<Presence, "who" | "name" | "pic">; onClick?: () => void; title?: string }) {
  return (
    <span class="avatar" style={{ "--who": colorOf(p.who) }} title={title ?? p.name} data-who={p.who} onClick={onClick}>
      {p.pic ? <img src={p.pic} alt="" referrerpolicy="no-referrer" /> : initials(p.name)}
    </span>
  );
}

/** One entry per person (their first client), not per window. */
function people(list: Presence[]): Presence[] {
  const seen = new Set<string>();
  return list.filter((p) => !seen.has(p.who) && seen.add(p.who));
}

/** Top bar: everyone else in this session. Click to follow them. */
export function PeopleBar({ client }: { client: Client }) {
  const tabs = new Set(client.state?.sessions.find((s) => s.id === client.session)?.tabs ?? []);
  const here = people(client.others().filter((p) => p.tab !== undefined && tabs.has(p.tab)));
  if (!here.length) return null;
  const following = client.state?.presence?.find((p) => p.client === client.following);
  return (
    <div class="people">
      {here.map((p) => (
        <Avatar
          key={p.who}
          p={p}
          title={following?.who === p.who ? `Following ${p.name} (click to stop)` : `${p.name}: click to follow`}
          onClick={() => client.follow(following?.who === p.who ? null : p.client)}
        />
      ))}
      {following ? <span class="following">following {following.name}</span> : null}
    </div>
  );
}

/** On a tab: who else is looking at it. */
export function TabPeople({ client, tab }: { client: Client; tab: number }) {
  const here = people(client.others().filter((p) => p.tab === tab));
  if (!here.length) return null;
  return (
    <span class="tab-people">
      {here.map((p) => (
        <span key={p.who} class="tab-dot" style={{ background: colorOf(p.who) }} title={p.name} />
      ))}
    </span>
  );
}

/** On a pane: an outline in the colour of each person focused on it, and
 * who drives it if that's someone else. */
export function PaneMarks({ client, pane }: { client: Client; pane: PaneId }) {
  if (client.info(pane)?.private && client.state?.roles) {
    return <div class="pane-private">Private: only its owner sees it</div>;
  }
  const focused = people(client.others().filter((p) => p.pane === pane));
  const driver = client.drivenBy(pane);
  const pair = client.info(pane)?.pair;
  // A guest who may only watch, said where they look (#664).
  const watching = !!client.state?.roles && client.role() === "viewer";
  // And one who has control of it: their typing reaches it.
  const driving = !!client.state?.roles && !watching && client.info(pane)?.driver?.who === client.me();
  if (!focused.length && !driver && !pair && !watching && !driving) return null;
  return (
    <>
      {focused.length ? <div class="pane-outline" style={{ "--who": colorOf(focused[0].who) }} /> : null}
      <div class="pane-people">
        {watching ? (
          <span class="pane-driver" data-you-watch title="Shared with you to watch: its owner can let you drive (Share, as someone who drives)">
            you watch
          </span>
        ) : null}
        {driving ? (
          <span class="pane-driver" data-you-drive title="You have control: your typing reaches it">
            ✎ you drive
          </span>
        ) : null}
        {focused.map((p) => (
          <span key={p.who} class="pane-person" style={{ "--who": colorOf(p.who) }}>
            {p.name.split("@")[0]}
          </span>
        ))}
        {driver ? (
          <>
            <span class="pane-driver" data-driver={driver.who} title={`${driver.name} is driving: only their typing reaches it`}>
              ✎ {driver.name.split("@")[0]}
            </span>
            {/* You may type here, but they have control: say how to get it (#551). */}
            {!pair && client.mayType(pane) ? (
              <button class="pane-take" data-take-control={pane} onClick={() => client.paneOp(pane, { op: "take_control" })}>
                Take control
              </button>
            ) : null}
          </>
        ) : pair ? (
          <span class="pane-driver" title="Pair mode: everyone types">
            ✎ pair
          </span>
        ) : null}
      </div>
    </>
  );
}

/** Pane menu: driving it, trusting, privacy (M13, M14). */
export function driveItems(client: Client, pane: PaneId): MenuItem[] {
  const info = client.info(pane);
  if (!info || info.type !== "terminal") return [];
  const owner = !client.state?.roles;
  const extra: MenuItem[] = [];
  if (owner) {
    extra.push({ label: "Private (only you see it)", checked: !!info.private, run: () => client.paneOp(pane, { op: "set_private", on: !info.private }) });
    for (const [who] of info.trusted ?? []) {
      extra.push({ label: `Stop trusting ${who.split(":").pop()?.split("@")[0]}`, run: () => client.paneOp(pane, { op: "revoke_trust", to: who }) });
    }
  } else if (client.role() !== "viewer" && !client.mayType(pane)) {
    extra.push({ label: "Ask the owner to let me drive it", run: () => client.paneOp(pane, { op: "request_trust" }) });
  }
  return [...driveItemsInner(client, pane, info), ...(extra.length ? ["separator" as const, ...extra] : [])];
}

function driveItemsInner(client: Client, pane: PaneId, info: NonNullable<ReturnType<Client["info"]>>): MenuItem[] {
  const others = client.others().length > 0;
  const driver = client.drivenBy(pane);
  const mine = info.driver?.who === client.me();
  const items: MenuItem[] = [];
  if (driver) {
    items.push({ label: `Take control (from ${driver.name.split("@")[0]})`, run: () => client.paneOp(pane, { op: "take_control" }) });
    items.push({ label: "Ask for control", run: () => client.paneOp(pane, { op: "request_control" }) });
  } else if (mine && others) {
    items.push({ label: "Let go of control", run: () => client.paneOp(pane, { op: "release_control" }) });
  }
  if (others || info.pair) {
    items.push({ label: "Pair mode (everyone types)", checked: !!info.pair, run: () => client.paneOp(pane, { op: "set_pair", on: !info.pair }) });
  }
  return items.length ? ["separator", ...items] : [];
}

/** A guest asks the owner to trust them with a pane on this machine. */
function TrustRequest({ client }: { client: Client }) {
  const r = client.trustRequests[0];
  const [minutes, setMinutes] = useState(30);
  if (!r) return null;
  return (
    <div class="prompt-backdrop">
      <div class="prompt" data-trust-request={r.pane}>
        <p>
          <b>{r.name}</b> asks to drive %{r.pane}. It runs on <b>this machine</b>, as you: they could do anything you can here.
        </p>
        <label class="share-history">
          For
          <select value={minutes} onChange={(e) => setMinutes(Number((e.target as HTMLSelectElement).value))}>
            <option value={10}>10 minutes</option>
            <option value={30}>30 minutes</option>
            <option value={120}>2 hours</option>
          </select>
        </label>
        <div class="prompt-buttons">
          <button onClick={() => client.answerTrust(r.pane, r.who, null)}>Not now</button>
          <button class="primary" data-trust onClick={() => client.answerTrust(r.pane, r.who, minutes)}>
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}

/** Someone asks to drive a pane you drive. */
export function ControlRequests({ client }: { client: Client }) {
  if (client.trustRequests.length) return <TrustRequest client={client} />;
  const r = client.requests[0];
  if (!r) return null;
  return (
    <div class="prompt-backdrop">
      <div class="prompt" data-control-request={r.pane}>
        <p>
          <b>{r.name}</b> asks to drive %{r.pane}.
        </p>
        <div class="prompt-buttons">
          <button onClick={() => client.answerRequest(r.pane, false)}>Not now</button>
          <button class="primary" data-give onClick={() => client.answerRequest(r.pane, true)}>
            Hand over
          </button>
        </div>
      </div>
    </div>
  );
}

interface Grant {
  session: SessionId;
  principal: string;
  name: string;
  role: Role;
  at: number;
  from?: Record<string, number>;
  /** An account's root, as the owner pinned it when sharing. */
  root?: string;
}

/** Someone on control, as `control.person` finds them. */
interface Person {
  account: string;
  name: string;
  root: string;
}

/** "Oct 1", with the year only when it isn't this one. */
const day = (ms: number) => {
  const d = new Date(ms);
  return d.toLocaleDateString(undefined, d.getFullYear() === new Date().getFullYear() ? { month: "short", day: "numeric" } : { year: "numeric", month: "short", day: "numeric" });
};

/** #763: verify someone you share with, if you want to: scan the code in
 * their Devices and machines…, or compare its words. Off the main path:
 * sharing pins their key on first use either way. */
function VerifyPerson({ who, done, close }: { who: Person; done: () => void; close: () => void }) {
  const [words, setWords] = useState<string[]>([]);
  const [scanning, setScanning] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    void verifyWords(who.account, who.root).then(setWords);
  }, [who.account, who.root]);
  const ok = () => {
    verifyPerson(who.account, who.root);
    done();
  };
  return (
    <div class="share-verify" data-verify={who.account}>
      <p>
        Scan the code in {who.name}'s <i>Devices and machines…</i>, or check it shows these words:
      </p>
      <p class="share-verify-words" data-verify-words>
        {words.join(" ")}
      </p>
      {scanning ? (
        <Scanner
          accept={(t) => {
            const c = readVerifyCode(t);
            if (!c) return false;
            if (c.account === who.account && c.root === who.root) ok();
            else setErr(`That code isn't the key control gave for ${who.name}. Don't share with them here.`);
            return true;
          }}
          close={() => setScanning(false)}
        />
      ) : null}
      {err ? <p class="share-warning">{err}</p> : null}
      <div class="prompt-buttons">
        <button onClick={close}>Cancel</button>
        {canScan() && !scanning ? (
          <button data-verify-scan onClick={() => setScanning(true)}>
            Scan their code
          </button>
        ) : null}
        <button class="primary" data-verify-match onClick={ok}>
          The words match
        </button>
      </div>
      <p class="dim">Inviting them to a team by link doesn't trust control at all.</p>
    </div>
  );
}

/** #663: granted logins seen on several devices, and tagged devices refused (none from an older daemon). */
interface Callers {
  shared: { login: string; devices: { device: string; at: number }[] }[];
  tagged: { device: string; tags: string[]; at: number }[];
}

/** Opened for one pane (#1473): the invite opens there, and "Watch me" is on. */
interface ShareAt {
  pane?: PaneId;
}

let openShare: ((s: SessionId, at?: ShareAt) => void) | null = null;
// Asked for before the dialog was there to open (#237: a card or a push
// that switched machines first): it opens once it is.
let pendingShare: { session: SessionId; at?: ShareAt } | null = null;

export function shareSession(session: SessionId, at?: ShareAt) {
  countView("share");
  if (openShare) openShare(session, at);
  else pendingShare = { session, at };
}

/** Share a session (M13): who has access, add someone, revoke. */
export function ShareDialog({ client }: { client: Client }) {
  const [session, setSession] = useState<SessionId | null>(null);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [callers, setCallers] = useState<Callers>({ shared: [], tagged: [] });
  // This machine's tailnet address, for a share to a tailnet login (#664).
  const [address, setAddress] = useState<string | null>(null);
  const [who, setWho] = useState("");
  const [role, setRole] = useState<Role>("viewer");
  // A team's role, picked beside its Share button (#551).
  const [teamRole, setTeamRole] = useState<Record<string, Role>>({});
  const [history, setHistory] = useState(false);
  const [err, setErr] = useState("");
  const [secrets, setSecrets] = useState<{ pane: PaneId; kinds: string[] }[]>([]);
  // Control mode (#763): someone whose key isn't the one first shared
  // with, stopped; and someone being verified, with what to do after.
  // `who` as the warning names them; `then`, what goes ahead once verified.
  const [changed, setChanged] = useState<{ p: Person; who: string; first?: number; seen: number; then?: () => void } | null>(null);
  const [verifying, setVerifying] = useState<{ p: Person; then?: () => void } | null>(null);
  // An invite (#233): what the notification says, and how it went.
  const [note, setNote] = useState("");
  // Where it opens, and whether their window follows ours (#1473).
  const [pane, setPane] = useState<PaneId | null>(null);
  const [watch, setWatch] = useState(false);
  const [told, setTold] = useState<{ delivery: string; text: string } | null>(null);
  const [link, setLink] = useState<string | null>(null);
  // After a share with someone outside your teams: what they see next.
  const [asked, setAsked] = useState<string | null>(null);
  const load = async (s: SessionId | null = session) => {
    const r = await client.request(OPS["acl.get"].method, opPath("acl.get"));
    if (r.ok) {
      const v = await r.json<{ grants: Grant[]; callers?: Callers }>();
      setGrants(v.grants);
      setCallers(v.callers ?? { shared: [], tagged: [] });
    }
    const h = await client.request(OPS["host.get"].method, opPath("host.get"));
    if (h.ok) setAddress((await h.json<{ tailnet_url?: string }>()).tailnet_url ?? null);
    if (s !== null) {
      const x = await client.request("GET", `/api/sessions/${s}/secrets`);
      if (x.ok) setSecrets(await x.json());
    }
  };
  useEffect(() => {
    const open = (s: SessionId, at?: ShareAt) => {
      setSession(s);
      setPane(at?.pane ?? null);
      setWatch(at?.pane !== undefined);
      setErr("");
      setAsked(null);
      void load(s);
    };
    openShare = open;
    if (pendingShare !== null) {
      const { session: s, at } = pendingShare;
      pendingShare = null;
      open(s, at);
    }
    // Only its own: when the page goes from its first, bare tree to the
    // workspace (M113), the dialog there opens before this one closes.
    return () => {
      if (openShare === open) openShare = null;
    };
  }, [client]);
  // As soon as it's drawn (an effect waits for the next frame), and before
  // the terminal, which keeps the keys it takes.
  useEscape("share.close", session === null ? null : () => setSession(null), [session]);
  if (session === null) return null;
  const name = client.state?.sessions.find((s) => s.id === session)?.name ?? `$${session}`;
  // Control mode: where a person it's shared with finds it (#551).
  const machine = control && client.e2e ? (control.daemons.find((d) => d.id === client.e2e!.daemon.id)?.name ?? "this machine") : null;
  const mine = grants.filter((g) => g.session === session);
  // The root a grant to an account holds, or this browser's pin for them.
  const rootOf = (g: Grant) => g.root ?? personPin(g.principal.slice("account:".length))?.root;
  // #763: a person whose key changed: control named another root than
  // the one pinned here (seen on a share), or this machine's grant holds
  // another. Then the key they'd be verified with, and the dates.
  const keyState = (g: Grant): { root: string; first?: number; seen: number } | null => {
    if (!g.principal.startsWith("account:")) return null;
    const account = g.principal.slice("account:".length);
    const pin = personPin(account);
    if (!pin) return null;
    const seen = keyChanged(account);
    if (seen) return { root: seen.root, first: pin.at, seen: seen.at };
    if (g.root && g.root !== pin.root) return { root: g.root, first: pin.at, seen: keyChangeSeen(account, g.root) };
    return null;
  };
  const verified = (g: Grant) => {
    if (!g.principal.startsWith("account:") || keyState(g)) return false;
    const p = personPin(g.principal.slice("account:".length));
    return !!p?.verified && p.root === rootOf(g);
  };
  // The warning for someone in the list, and what goes ahead once they're
  // verified: the grant, at `role`, with the key verified.
  const warnFor = (g: Grant, k: { root: string; first?: number; seen: number }, role: Role = g.role) =>
    setChanged({
      p: { account: g.principal.slice("account:".length), name: g.name, root: k.root },
      who: g.name,
      first: k.first,
      seen: k.seen,
      then: () => void set(g.principal, role, true, { root: k.root, name: g.name }),
    });
  const set = async (principal: string, r: Role | null, withHistory = true, extra: Record<string, string> = {}) => {
    const res = await client.request(OPS["acl.set"].method, opPath("acl.set"), { session, principal, role: r, history: withHistory, ...extra });
    if (!res.ok) setErr((await res.json<{ error?: string }>().catch(() => null))?.error ?? `HTTP ${res.status}`);
    await load();
  };
  // Share and notify (#233): the machine grants and pushes them alone. A
  // name it doesn't know is looked up on control and shared with as a
  // plain share is (`offer`).
  const invite = async (whom: string, extra: Record<string, string> = {}) => {
    setErr("");
    setTold(null);
    const res = await client.request(OPS["invite.send"].method, opPath("invite.send"), { session, who: whom, role, history, note, pane, follow: watch, ...extra } satisfies InviteRequest);
    const body = await res.json<Partial<Invited> & { error?: string }>().catch(() => null);
    if (res.status === 404 && control && client.e2e && !extra.root) {
      control.person(whom).then((p) => offer(p, whom, true), (x: Error) => setErr(x.message));
      return;
    }
    if (!res.ok || !body?.delivery) {
      setErr(body?.error ?? `HTTP ${res.status}`);
      return;
    }
    const n = body.grant?.name ?? whom;
    const why = body.reason ? `: ${body.reason}` : "";
    const text = body.delivery === "sent" ? `${n} was notified` : body.delivery === "pending" ? `${n} will be notified${why}` : `${n} wasn't notified${why}`;
    setTold({ delivery: body.delivery, text });
    setWho("");
    setNote("");
    await load();
  };
  const share = async (p: Person, notify: boolean) => {
    await (notify ? invite(`account:${p.account}`, { root: p.root }) : set(`account:${p.account}`, role, history, { root: p.root, name: p.name }));
    setAsked(`If ${p.name} isn't in one of your teams, they're asked first. Then it's under ${machine ?? "this machine"} in their host menu.`);
    setWho("");
  };
  // #763: trust on first use. The first share pins their root in this
  // browser; after that, a root control names that isn't the one pinned
  // here, or the one a grant on this machine holds (shared from another
  // device of yours), stops the share.
  const offer = async (p: Person, login: string, notify: boolean, go: () => Promise<void> = () => share(p, notify)) => {
    setChanged(null);
    setVerifying(null);
    const held = grants.find((g) => g.principal === `account:${p.account}` && g.root);
    // A grant on this machine counts as a pin, from when it was made.
    if (held?.root) pinPerson(p.account, held.root, held.at);
    const pinned = personPin(p.account);
    if (pinned && pinned.root !== p.root) {
      const who = /\s/.test(login) ? p.name : `@${login.replace(/^@/, "")}`;
      setChanged({ p, who, first: pinned.at, seen: keyChangeSeen(p.account, p.root), then: () => void go() });
      return;
    }
    pinPerson(p.account, p.root);
    await go();
  };
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && setSession(null)}>
      <div class="prompt share-dialog" data-share={session}>
        <h2>Share {name}</h2>
        {mine.length ? (
          <ul class="share-list">
            {mine.map((g) => {
              const k = keyState(g);
              const account = g.principal.startsWith("account:") ? g.principal.slice("account:".length) : null;
              return (
                <li key={g.principal} data-grant={g.principal}>
                  <Avatar p={{ who: g.principal, name: g.name }} />
                  <span class="share-name" title={g.name}>
                    {g.name}
                    {verified(g) ? (
                      <span class="share-verified" title="You verified their key" aria-label="verified" data-verified>
                        {" "}
                        ✓
                      </span>
                    ) : null}
                  </span>
                  <select
                    value={g.role}
                    onChange={(e) => {
                      const el = e.target as HTMLSelectElement;
                      const r = el.value as Role;
                      // A changed key stops a role change as it stops a share.
                      if (k) {
                        el.value = g.role;
                        warnFor(g, k, r);
                      } else void set(g.principal, r);
                    }}
                  >
                    <option value="viewer">{roleLabel("viewer")}</option>
                    <option value="editor">{roleLabel("editor")}</option>
                    <option value="owner">{roleLabel("owner")}</option>
                  </select>
                  <span class="dim" title={g.from ? "Shared from now on: no history" : "Shared with its history"}>
                    {g.from ? "from now" : "history"}
                  </span>
                  {/* Always a cell, so Remove lines up whatever is here. */}
                  <span class="share-key">
                    {control && account && k ? (
                      <button class="control-linkish share-warning" data-key-changed-row={g.principal} title="Their account key changed" onClick={() => warnFor(g, k)}>
                        ⚠ new key
                      </button>
                    ) : control && account && !verified(g) && rootOf(g) ? (
                      <button class="control-linkish" data-verify-person={g.principal} onClick={() => setVerifying({ p: { account, name: g.name, root: rootOf(g)! } })}>
                        Verify
                      </button>
                    ) : null}
                  </span>
                  <button class="control-revoke quiet-danger" onClick={() => void set(g.principal, null)}>
                    Remove
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p class="dim">Only you can reach it.</p>
        )}
        {address && !(control && client.e2e) && mine.some((g) => g.principal.startsWith("tailnet:")) ? (
          <p class="dim" data-share-address>
            Send them this machine's tailnet address, where they open it: <CopyText text={address} inline share data-share-url />
          </p>
        ) : null}
        {callers.shared
          .filter((c) => mine.some((g) => g.principal === `tailnet:${c.login}`))
          .map((c) => (
            <p key={c.login} class="share-warning" data-shared-login={c.login}>
              {c.login} is signed in on {c.devices.length} devices ({c.devices.map((d) => d.device).join(", ")}): everyone on them has
              this role, and Arugula can't tell them apart.
            </p>
          ))}
        {callers.tagged.map((t) => (
          <p key={t.device} class="dim" data-tagged-caller={t.device}>
            {t.device} ({t.tags.join(", ")}) is a tagged device: it has no login, so it was refused and nothing can be shared with it.
          </p>
        ))}
        {secrets
          .filter((x) => !client.info(x.pane)?.private)
          .map((x) => (
            <p key={x.pane} class="share-warning" data-secret={x.pane}>
              ⚠ %{x.pane} shows what looks like {x.kinds.join(" and ")} (a guess).{" "}
              <button
                class="control-linkish"
                onClick={() => {
                  client.paneOp(x.pane, { op: "set_private", on: true });
                  setSecrets(secrets.filter((y) => y.pane !== x.pane));
                }}
              >
                Make it private
              </button>
            </p>
          ))}
        <p class="dim" data-share-note>
          {client.has("vms")
            ? "People you share with open their own panes on throwaway VMs."
            : "People you share with see its panes but can't open their own here."}{" "}
          {machine
            ? `Someone outside your teams accepts it first; then it's under ${machine} in their host menu. Your teams aren't asked. `
            : ""}
          Unless this machine is in their team, even people who drive ask you before typing in one of its panes.
        </p>
        {changed ? (
          <div class="share-changed" data-key-changed={changed.p.account}>
            <p class="share-warning">
              <b>⚠ {changed.who}'s account key changed since you first shared with them.</b>
            </p>
            <p class="dim">
              {changed.first ? `You first shared on ${day(changed.first)}. ` : ""}Control has given a different key since {day(changed.seen)}. Nothing was
              shared.
            </p>
            <div class="prompt-buttons">
              <button onClick={() => setChanged(null)}>Cancel</button>
              <button
                class="primary"
                data-key-verify
                onClick={() => {
                  const c = changed;
                  setChanged(null);
                  setVerifying({ p: c.p, then: c.then });
                }}
              >
                Verify {changed.p.name}
              </button>
            </div>
          </div>
        ) : null}
        {verifying ? (
          <VerifyPerson
            who={verifying.p}
            close={() => setVerifying(null)}
            done={() => {
              const then = verifying.then;
              setVerifying(null);
              then?.();
            }}
          />
        ) : null}
        {control && client.e2e
          ? control.teams
              .filter((t) => t.verified && t.role !== null && !mine.some((g) => g.principal === `team:${t.team}`))
              .map((t) => (
                <p key={t.team} class="share-team">
                  <button
                    data-share-team={t.team}
                    onClick={() =>
                      // Pinned to its founder, as this browser pinned the
                      // team: the machine checks every roster back to them.
                      void set(`team:${t.team}`, teamRole[t.team] ?? "viewer", history, {
                        root: `${t.pin.founder}.${t.pin.founder_root}`,
                        name: t.roster.name,
                      })
                    }
                  >
                    Share with everyone in {t.roster.name}
                  </button>{" "}
                  <select
                    value={teamRole[t.team] ?? "viewer"}
                    onChange={(e) => setTeamRole({ ...teamRole, [t.team]: (e.target as HTMLSelectElement).value as Role })}
                    aria-label={`Role for ${t.roster.name}`}
                    data-share-team-role={t.team}
                  >
                    <option value="viewer">{roleLabel("viewer")}</option>
                    <option value="editor">{roleLabel("editor")}</option>
                  </select>{" "}
                  <span class="dim">(as members come and go)</span>
                </p>
              ))
          : null}
        <form
          class="share-add"
          onSubmit={(e) => {
            e.preventDefault();
            const p = who.trim();
            if (!p) return;
            if (control && client.e2e) {
              control.person(p).then((x) => offer(x, p, false), (x: Error) => setErr(x.message));
              return;
            }
            void set(p.includes(":") ? p : `tailnet:${p}`, role, history).then(() => setWho(""));
          }}
        >
          <input
            placeholder={control && client.e2e ? "their name or login on Arugula" : "their tailnet login"}
            value={who}
            onInput={(e) => setWho((e.target as HTMLInputElement).value)}
            aria-label="Who"
          />
          <select value={role} onChange={(e) => setRole((e.target as HTMLSelectElement).value as Role)} aria-label="Role">
            <option value="viewer">{roleLabel("viewer")}</option>
            <option value="editor">{roleLabel("editor")}</option>
          </select>
          <label class="share-history">
            <input type="checkbox" checked={history} onChange={(e) => setHistory((e.target as HTMLInputElement).checked)} /> with history
          </label>
          <button type="submit" class="primary">
            Share
          </button>
          <input
            class="share-note"
            placeholder="a note, to notify them"
            value={note}
            onInput={(e) => setNote((e.target as HTMLInputElement).value)}
            aria-label="Note"
            data-invite-note
          />
          <label class="share-history" title="Their window follows yours when they open the invite">
            <input type="checkbox" data-invite-watch checked={watch} onChange={(e) => setWatch((e.target as HTMLInputElement).checked)} /> Watch me
          </label>
          <button
            type="button"
            data-invite
            onClick={() => {
              const p = who.trim();
              if (!p) return;
              // A login is a tailnet one here; a name, someone the machine knows.
              if (!(control && client.e2e)) return void invite(p.includes("@") && !p.includes(":") ? `tailnet:${p}` : p);
              // Control mode: their key checked first, as for Share (#763),
              // then by name, so a teammate the machine knows is named as
              // it knows them.
              control.person(p).then(
                (x) => offer(x, p, true, () => invite(p)),
                () => void invite(p),
              );
            }}
          >
            Share and notify
          </button>
        </form>
        {asked ? (
          <p class="dim" data-share-asked>
            {asked}
          </p>
        ) : null}
        {told ? (
          <p class="dim" data-invite-delivery={told.delivery}>
            {told.text}
          </p>
        ) : null}
        {control && client.e2e ? (
          <p>
            <button
              class="control-linkish"
              data-make-link
              onClick={() =>
                control!
                  .makeLink((m, p, b) => client.request(m, p, b), client.e2e!.daemon.id, session, 3600, false)
                  .then(setLink, (x: Error) => setErr(x.message))
              }
            >
              Make a read-only link
            </button>{" "}
            <span class="dim">(anyone with it watches, from now on, for an hour; no account needed)</span>
          </p>
        ) : null}
        {link ? <CopyText text={link} share data-link /> : null}
        {err ? <p class="control-error">{err}</p> : null}
        <div class="prompt-buttons">
          <button onClick={() => setSession(null)}>Done</button>
        </div>
      </div>
    </div>
  );
}
