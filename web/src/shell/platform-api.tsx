// The shell's side of the platform's views (M147): what a pane's thread
// (../shared/chat/) reaches the shell through, built on this page's fleet
// and control session. The views never import any of it; they take a
// `PlatformApi` (../shared/platform-api). Chat's page is the
// platform page's (M153), and the shell doesn't draw it since M155: its
// links go there.

import { useState } from "preact/hooks";
import type { Client } from "./client";
import type { FleetPane } from "./fleet";
import { directory } from "./hosts";
import { whatItDoes } from "./swarm/model";
import { openPlatform, platformHref } from "./platform-links";
import { getFleet } from "./ui/hosts";
import { getControlSession } from "./ui/people";
import { openChat, openChatView, openRoom } from "./ui/places";
import { closeRoomDrawer } from "./ui/threads";
import type { PaneQuote } from "../shared/chat-store";
import type { PlatformApi } from "../shared/platform-api";
import { useSubscribe } from "../shared/ui/hooks";

const made = new WeakMap<Client, PlatformApi>();

/** The platform API on this page: one per client. */
export function shellPlatformApi(client: Client): PlatformApi {
  let api = made.get(client);
  if (!api) made.set(client, (api = build(client)));
  return api;
}

const hostOf = (id: string | null | undefined) => (id ? getFleet()?.hostById(id) : undefined);

/** The pane as the fleet knows it, when it does. */
function fleetPane(machine: string | null | undefined, pane: number | string): FleetPane | undefined {
  const h = hostOf(machine);
  return h ? getFleet()?.panes.find((p) => p.host === h.name && p.id === Number(pane)) : undefined;
}

function build(client: Client): PlatformApi {
  const toast = (s: string) => client.toast(s);
  return {
    teams: () => getControlSession()?.teams ?? [],
    subscribe(fn) {
      const offs = [getFleet()?.subscribe(fn), getControlSession()?.subscribe(fn)];
      return () => offs.forEach((off) => off?.());
    },
    toast,
    // Chat is on the platform page: these open it there.
    nav: {
      openChat,
      closeChat: () => {},
      openRoom,
      openChatView,
      // The platform page's address for it, else the shell's old one, which
      // goes there once control has a platform page (M154).
      openGrants: () => openPlatform("/settings"),
      roomHref: (room) => platformHref(`/chat/${room}`) ?? `#chat=${room}`,
      roomLink: (room, seq, thread) => {
        const on = thread !== undefined ? `&thread=${thread}` : "";
        return platformHref(`/chat/${room}?msg=${seq}${on}`) ?? `${location.origin}${location.pathname}${location.search}#chat=${room}&msg=${seq}${on}`;
      },
    },
    machine(id) {
      const h = hostOf(id);
      return h ? { name: h.name, up: h.state === "connected" } : undefined;
    },
    paneTitle(machine, pane) {
      const p = fleetPane(machine, pane);
      return p ? whatItDoes(p) : "";
    },
    sessionName(machine, session) {
      return hostOf(machine)?.summary?.sessions.find((x) => String(x.id) === String(session))?.name ?? "";
    },
    goToPane(machine, pane, quote) {
      const fleet = getFleet();
      const h = fleet?.hostById(machine);
      if (!fleet || !h) return toast("That machine isn't one of yours to open");
      if (h.state !== "connected") return toast(`${h.name} is asleep: the quote is all there is until it's back`);
      closeRoomDrawer();
      fleet.open(h.name, Number(pane), false, quote ?? undefined);
    },
    goToSession(machine) {
      const h = hostOf(machine);
      if (!h || h.state !== "connected") return toast(`${h?.name ?? "That machine"} is asleep`);
      getFleet()?.touch(h.name);
      directory.select(h.name);
    },
    // Chat's page, which draws the bar, isn't the shell's (M155).
    Bar: ({ children }) => <>{children}</>,
    quote: { can: () => (getFleet()?.list.length ?? 0) > 0, Picker: QuotePicker },
  };
}

/** Pick a pane on a machine that's up, read its screen end to end, and
 * keep the lines you want: they're copied into the message. */
export function QuotePicker({ close, take }: { close: () => void; take: (q: PaneQuote) => void }) {
  const fleet = getFleet();
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const [picked, setPicked] = useState<FleetPane | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const panes = (fleet?.panes ?? []).filter((p) => !p.stale && fleet?.host(p.host)?.id);
  const choose = async (p: FleetPane) => {
    setPicked(p);
    setText(null);
    setErr("");
    try {
      const res = await fleet!.request(p.host, "GET", `/api/panes/${p.id}/capture?format=text`);
      if (!res.ok) throw new Error(`it said ${res.status}`);
      const lines = (res.text ? await res.text() : "").split("\n").map((l) => l.trimEnd());
      while (lines.length && !lines[lines.length - 1]) lines.pop();
      setText(lines.slice(-20).join("\n"));
    } catch (e) {
      setErr(`Couldn't read ${p.host}'s pane: ${(e as Error).message}`);
    }
  };
  return (
    <div class="chat-quote-pick" role="dialog" aria-label="Quote a pane" data-quote-picker>
      <header>
        <strong>{picked ? `%${picked.id} on ${picked.host}` : "Quote a pane"}</strong>
        <button type="button" class="thread-close" title="Close" aria-label="Close the pane picker" onClick={close}>
          ✕
        </button>
      </header>
      {!picked && (
        <ul>
          {panes.length === 0 && <li class="chat-none">No machine of yours is up to quote from.</li>}
          {panes.map((p) => (
            <li key={p.key}>
              <button type="button" data-quote-from={`${p.host}:${p.id}`} onClick={() => void choose(p)}>
                <span class="chat-sigil">↳</span>%{p.id} {whatItDoes(p)} <span class="dim">· {p.host}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {picked && (
        <>
          {text === null && !err && <p class="chat-none">Reading it…</p>}
          {text !== null && (
            <>
              <p class="dim">Keep the lines you want to share: only these go to control, in the message.</p>
              <textarea class="chat-quote-text" aria-label="Lines to quote" value={text} rows={8} onInput={(e) => setText((e.target as HTMLTextAreaElement).value)} />
            </>
          )}
          {err && <p class="thread-error">{err}</p>}
          <div class="thread-offer-buttons">
            <button
              type="button"
              class="primary"
              data-quote-take
              disabled={!text?.trim()}
              onClick={() => take({ machine: fleet!.host(picked.host)!.id!, pane: picked.id, text: text!.trimEnd() })}
            >
              Quote these lines
            </button>
            <button type="button" onClick={() => setPicked(null)}>
              Another pane
            </button>
          </div>
        </>
      )}
    </div>
  );
}
