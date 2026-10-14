// Links (#856): a conversation's panes, here and on other machines. A link
// is made in two steps, on two machines: the pane's machine keeps a grant
// (made by someone who may drive the pane, with their rights on it), then
// the conversation's home keeps the link. This is *Add to conversation…*
// in the pane menu, the pane's own list of its grants (with Revoke), and
// the chips in a conversation's header.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { directory } from "../hosts";
import { CONV_MEMBERS_SEE, type ConvGrant, type ConvLink, type ConvLinkState, type ConvLinkStatus, type Conversation, type PaneId } from "../proto";
import type { MenuItem } from "./menu";
import { getFleet } from "./hosts";
import { sources, type Source } from "./places";

type Open = { kind: "add"; client: Client; pane: PaneId } | { kind: "grants"; client: Client; pane: PaneId };

let open: Open | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
const show = (o: Open | null) => {
  open = o;
  changed();
};

/** The pane menu's link items: for someone who may drive the pane, on a
 * machine with chat. */
export function linkItems(client: Client, pane: PaneId): MenuItem[] {
  if (!client.hasThreads() || !client.flag("chat") || !client.mayType(pane)) return [];
  return [
    { label: "Add to conversation…", run: () => show({ kind: "add", client, pane }) },
    { label: "Linked conversations…", run: () => show({ kind: "grants", client, pane }) },
  ];
}

export function LinkLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  if (!open) return null;
  const close = () => show(null);
  return open.kind === "add" ? <AddToConv client={open.client} pane={open.pane} close={close} /> : <PaneGrants client={open.client} pane={open.pane} close={close} />;
}

/** What to call a pane in a link: its title, else its directory. */
function paneTitle(client: Client, pane: PaneId): string {
  return client.title(pane) || client.cwd(pane)?.split("/").filter(Boolean).pop() || `%${pane}`;
}

/** A host's control device id, if it has one. */
const deviceOf = (host: string): string | undefined => directory.find(host)?.id;

/** The host a link's machine is, by its device id (none: the home). */
export function hostOfLink(home: Source, l: ConvLink): string | undefined {
  if (!l.machine) return home.host;
  return directory.list?.hosts.find((h) => h.id === l.machine)?.name;
}

/** Everyone in a conversation but you, by name. */
function othersIn(c: Conversation, me: string): string[] {
  return [{ who: c.owner, name: c.owner_name || "owner" }, ...c.members].filter((m) => m.who !== me).map((m) => m.name);
}

function names(list: string[]): string {
  if (list.length <= 1) return list.join("");
  return `${list.slice(0, -1).join(", ")} and ${list.at(-1)}`;
}

/** Grant on the pane's machine, then link at the conversation's home. A
 * link the home refuses takes its grant back. */
async function link(client: Client, pane: PaneId, home: Source, conv: Conversation): Promise<void> {
  const here = directory.current;
  const same = home.shown;
  const homeId = same ? null : deviceOf(home.host);
  const paneMachine = same ? null : deviceOf(here);
  if (!same && (!homeId || !paneMachine)) throw new Error(`${home.host} and ${here || "this machine"} need to be in Arugula control to link across them`);
  const g = await client.grantConv(pane, { device: homeId, device_name: same ? null : home.host, conv: conv.id, conv_title: conv.title });
  try {
    await home.client.linkConv(conv.id, { machine: paneMachine, pane, epoch: g.epoch, title: paneTitle(client, pane) });
  } catch (e) {
    await client.revokeConvGrant(pane, g.device, conv.id).catch(() => {});
    throw e;
  }
}

/** Escape closes a dialog, before anything under it hears the key. */
function useEscape(close: () => void) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, []);
}

function AddToConv({ client, pane, close }: { client: Client; pane: PaneId; close: () => void }) {
  const all = sources(client, getFleet());
  const choices = all.flatMap((s) => s.client.convs().map((c) => ({ s, c })));
  const [picked, setPicked] = useState<string | null>(null);
  const [making, setMaking] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (making) input.current?.focus();
  }, [making]);
  useEscape(close);
  const chosen = choices.find((x) => `${x.s.host}/${x.c.id}` === picked);
  const linked = (c: Conversation) => c.links.some((l) => !l.closed && l.pane === pane && (l.machine ?? null) === (chosenMachine(c) ?? null));
  // The pane's machine as a link names it from that conversation's home.
  function chosenMachine(c: Conversation): string | null {
    const s = choices.find((x) => x.c.id === c.id)?.s;
    return !s || s.shown ? null : (deviceOf(directory.current) ?? null);
  }
  const warnAbout = chosen ? othersIn(chosen.c, chosen.s.client.me()) : [];
  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      if (making) {
        const c = await client.createConv(title);
        await link(client, pane, { host: directory.current, client, shown: true }, c);
      } else if (chosen) {
        await link(client, pane, chosen.s, chosen.c);
      }
      client.toast(`%${pane} is linked`);
      close();
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && close()}>
      <form
        class="prompt conv-dialog"
        aria-label="Add to conversation"
        data-link-dialog
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) void go();
        }}
      >
        <h2>Add {paneTitle(client, pane)} to a conversation</h2>
        {choices.length === 0 && !making && <p class="conv-note">You're in no conversations yet.</p>}
        <ul class="link-choices">
          {choices.map(({ s, c }) => {
            const key = `${s.host}/${c.id}`;
            const done = linked(c);
            return (
              <li key={key}>
                <label class="control-check">
                  <input
                    type="radio"
                    name="link-conv"
                    checked={!making && picked === key}
                    disabled={done}
                    data-link-conv={c.id}
                    onChange={() => {
                      setMaking(false);
                      setPicked(key);
                    }}
                  />{" "}
                  {c.title}
                  <span class="dim"> · {s.host || "this machine"}{done ? " · linked already" : ""}</span>
                </label>
              </li>
            );
          })}
          {client.ownsMachine() && (
            <li>
              <label class="control-check">
                <input type="radio" name="link-conv" checked={making} data-link-new onChange={() => setMaking(true)} /> New…
              </label>
            </li>
          )}
        </ul>
        {making && (
          <label>
            Title
            <input ref={input} value={title} maxLength={100} placeholder="calc tests" data-link-title onInput={(e) => setTitle((e.target as HTMLInputElement).value)} />
          </label>
        )}
        {!making && warnAbout.length > 0 && (
          <p class="conv-warning" data-link-warning>
            {names(warnAbout)} will be able to drive this pane through @lead. {CONV_MEMBERS_SEE}
          </p>
        )}
        <p class="conv-note">The conversation can prompt this pane's agent, wait on it, read its screen and answer its cards, with your rights on it. You can revoke that from the pane's menu.</p>
        {error && <p class="thread-error">{error}</p>}
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" data-link-go disabled={busy || (making ? !title.trim() : !chosen)}>
            Link
          </button>
        </div>
      </form>
    </div>
  );
}

/** A pane's grants: the conversations that may drive it, each revocable. */
function PaneGrants({ client, pane, close }: { client: Client; pane: PaneId; close: () => void }) {
  const [grants, setGrants] = useState<ConvGrant[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    client.convGrants(pane).then(setGrants, (e: Error) => setError(e.message));
  }, [pane]);
  useEscape(close);
  const revoke = async (g: ConvGrant) => {
    setError(null);
    try {
      setGrants(await client.revokeConvGrant(pane, g.device, g.conv));
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt conv-dialog" aria-label="Linked conversations" data-grants-dialog>
        <h2>Conversations linked to {paneTitle(client, pane)}</h2>
        {grants === null && !error && <p class="conv-note">Looking…</p>}
        {grants?.length === 0 && <p class="conv-note" data-grants-none>None. *Add to conversation…* links one.</p>}
        {grants && grants.length > 0 && (
          <ul class="chat-people" data-grants>
            {grants.map((g) => (
              <li key={`${g.device}/${g.conv}`} data-grant-conv={g.conv}>
                <span>{g.conv_title || g.conv}</span>
                <span class="dim">
                  {g.device_name || "this machine"} · by {g.by_name || g.by}
                </span>
                <button class="conv-remove" title="It can no longer drive this pane" data-revoke-grant={g.conv} onClick={() => void revoke(g)}>
                  Revoke
                </button>
              </li>
            ))}
          </ul>
        )}
        {error && <p class="thread-error">{error}</p>}
        <div class="prompt-buttons">
          <button type="button" class="primary" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

const STATE_WORD: Record<ConvLinkState, string> = { live: "live", closed: "closed", offline: "offline" };

type LeadState = "working" | "waiting" | "idle" | "gone" | "none";
const LEAD_WORD: Record<LeadState, string> = {
  working: "working",
  waiting: "needs you",
  idle: "idle",
  gone: "gone: start another",
  none: "start one",
};

/** The conversation's lead (#858), first among its chips: what it's doing,
 * opening its block; or, when it's gone or never started, a button that
 * starts one (the conversation keeps its id and timeline). */
function LeadChip({ s, conv, go }: { s: Source; conv: Conversation; go: (block: PaneId) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const block = conv.lead?.block;
  const info = block === undefined ? undefined : s.client.info(block);
  const state: LeadState = !conv.lead
    ? "none"
    : !info
      ? "gone"
      : info.attention === "working"
        ? "working"
        : info.attention === "needs_input"
          ? "waiting"
          : "idle";
  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      await s.client.startLead(conv.id);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const live = state !== "gone" && state !== "none";
  return (
    <span class={`link-chip lead ${state}`} data-conv-lead data-lead-state={state}>
      {live ? (
        <button
          class="link-chip-open"
          title={`The lead: an agent here that drives the links with the owner's rights. Write @lead to ask it (${LEAD_WORD[state]})`}
          onClick={() => block !== undefined && go(block)}
        >
          <span class="link-chip-dot" aria-hidden="true" />
          @lead · {LEAD_WORD[state]}
        </button>
      ) : (
        <button class="link-chip-open" title="Start the conversation's lead" data-start-lead disabled={busy} onClick={() => void start()}>
          <span class="link-chip-dot" aria-hidden="true" />
          @lead · {busy ? "starting…" : LEAD_WORD[state]}
        </button>
      )}
      {error && <span class="thread-error">{error}</span>}
    </span>
  );
}

/** A conversation's lead and links, as chips in its header: each link with
 * its state (asked of its home every 10 s while shown). `go` opens a link,
 * `goLead` the lead's block. */
export function LinkChips({ s, conv, go, goLead }: { s: Source; conv: Conversation; go: (l: ConvLink) => void; goLead: (block: PaneId) => void }) {
  const [states, setStates] = useState<Map<string, ConvLinkStatus>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const open = conv.links.filter((l) => !l.closed).length;
  useEffect(() => {
    if (!open) return;
    let live = true;
    const read = () =>
      s.client.convLinks(conv.id).then(
        (r) => live && setStates(new Map(r.map((x) => [x.name, x]))),
        () => {},
      );
    void read();
    const t = setInterval(read, 10_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [s.host, conv.id, open]);
  const me = s.client.me();
  const unlink = async (l: ConvLink) => {
    setError(null);
    try {
      await s.client.unlinkConv(conv.id, l.name);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <div class="link-chips" data-conv-links>
      <LeadChip s={s} conv={conv} go={goLead} />
      {conv.links.map((l) => {
        const st = states.get(l.name);
        const state: ConvLinkState = l.closed ? "closed" : (st?.state ?? "live");
        const why = l.closed ?? st?.why;
        const where = l.machine_name || (l.machine ? (hostOfLink(s, l) ?? "another machine") : s.host || "this machine");
        return (
          <span key={l.name} class={`link-chip ${state}`} data-conv-link={l.name} data-link-state={state} data-link-known={st || l.closed ? "1" : "0"}>
            <button class="link-chip-open" title={`${l.title} on ${where}: ${STATE_WORD[state]}${why ? ` (${why})` : ""}`} onClick={() => go(l)}>
              <span class="link-chip-dot" aria-hidden="true" />
              {l.name} · {l.title || `%${l.pane}`} · {where}
            </button>
            {!l.closed && (conv.owner === me || l.by === me) && (
              <button class="link-chip-close" title={`Unlink ${l.name}`} aria-label={`Unlink ${l.name}`} data-unlink={l.name} onClick={() => void unlink(l)}>
                ✕
              </button>
            )}
          </span>
        );
      })}
      {error && <span class="thread-error">{error}</span>}
    </div>
  );
}
