// Over the panes, the workspace's top bar (M108, #1023; the kit's since Lift
// 3, #1479) says where you are: the machine, the session and the tab, as a
// crumb; then how it's reached, its places, the bell and the people here;
// then Go to… and Share. It's the desktop app's titlebar.

import { tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import { useSubscribe } from "../../../shared/ui/hooks";
import { getFleet, HostButton, hostMenuShown, machineState } from "../hosts";
import { PeopleBar, shareSession } from "../people";
import { UpdateChip } from "../update";
import type { Step } from "../../../shared/kit/crumb";
import { TopBar } from "../../../shared/kit/frame/top";
import type { Renaming } from "./rename-input";
import { SessionButton } from "./session-sidebar";
import { offlineWhy } from "./live";
import { setSidebarOpen, useSidebar } from "./prefs";
import { usePlace } from "./rail";
import { ChevronIcon } from "../../../shared/ui/icons";

export type Connection = "direct" | "relayed" | "reconnecting" | "offline" | "waking" | "asleep";

/** How the shown machine is reached, from real state: the page's own
 * connection first (direct or through control's relay), then, while it's
 * down, what control and the fleet say of the machine. */
export function connection(client: Client): { state: Connection; why: string } {
  if (client.connected) {
    return client.path === "relayed"
      ? { state: "relayed", why: "Through Arugula cloud's relay, which can read what it carries and keeps none of it" }
      : { state: "direct", why: "Straight to the machine" };
  }
  const name = directory.current;
  // M182: a hosted machine's box, as control says.
  if (client.box?.state === "waking") return { state: "waking", why: `Waking ${name || "the machine"}: its box was paused` };
  if (client.box?.state === "asleep") return { state: "asleep", why: `${name || "The machine"} is asleep: its box is paused until it's opened` };
  if (client.box?.state === "failed") return { state: "offline", why: (client.box.why ?? `${name || "The machine"} didn't wake`).replace(/\.$/, "") };
  const said = name ? machineState(name) : "";
  if (said.startsWith("offline")) return { state: "offline", why: `${name} is ${said}; still trying` };
  return { state: "reconnecting", why: `Lost the connection to ${name || "the machine"}; reconnecting${client.failures ? ` (${client.failures} tries)` : ""}` };
}

const CONNECTION_WORDS: Record<Connection, string> = {
  direct: "Direct",
  relayed: "Relayed",
  reconnecting: "Reconnecting",
  offline: "Offline",
  waking: "Waking",
  asleep: "Asleep",
};

export function ConnectionChip({ client }: { client: Client }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => getFleet()?.subscribe(fn) ?? (() => {}));
  const { state, why } = connection(client);
  return (
    <span class={`ws-conn ${state}`} data-connection={state} role="status" title={why}>
      <span class="ws-dot" aria-hidden="true" />
      {CONNECTION_WORDS[state]}
    </span>
  );
}

/** Shows and hides the sessions sidebar (#378). It's the tab strip's first
 * thing, at the sidebar's edge, so it stays when the sidebar is closed. The
 * panes only: the other places cover the sidebar. */
export function SidebarToggle({ client }: { client: Client }) {
  const at = usePlace(client);
  const { open } = useSidebar();
  if (at !== "panes") return null;
  const label = `${open ? "Hide" : "Show"} sessions sidebar`;
  return (
    <button
      class="ws-sidebar-toggle"
      data-open-sessions
      aria-label={label}
      aria-controls="ws-session-sidebar"
      aria-expanded={open}
      title={`${label} on ${directory.current || "this machine"}`}
      onClick={() => setSidebarOpen(!open)}
    >
      <ChevronIcon dir={open ? "left" : "right"} />
    </button>
  );
}

/** The workspace's header. */
export function ContextHeader({
  client,
  setRenaming,
}: {
  client: Client;
  setRenaming: (r: Renaming) => void;
}) {
  useSubscribe((fn) => directory.subscribe(fn));
  const at = usePlace(client);
  // The session's own parts (where you are in it, Share) are the panes'.
  const panes = at === "panes";
  const state = client.state!;
  // The shown project's only (Arugula #1347): none when it has none here.
  const shown = client.shownSessions();
  const session = shown.find((s) => s.id === client.session) ?? shown[0];
  const tab = client.tabView();
  // Machine › session › tab: the machine and the session are menus of their
  // own; the machine's name even where there's no menu of others (a
  // daemon's own page alone: HostButton draws nothing then).
  const steps: Step[] = [
    {
      label: directory.current || client.hostName || "This machine",
      hook: "machine",
      node: hostMenuShown() ? <HostButton path={false} /> : undefined,
    },
  ];
  if (panes && session) steps.push({ label: session.name, hook: "session", node: <SessionButton client={client} session={session} setRenaming={setRenaming} /> });
  if (panes && session && tab) steps.push({ label: tabLabel(client, tab), hook: "tab" });
  // How it's reached beside the crumb (the rail has the places, Lift 6a);
  // an update, what waits and who's here on the right, past the empty
  // middle (the window's drag, desktop-xvfb).
  const views = <ConnectionChip client={client} />;
  const tools = (
    <>
      <UpdateChip client={client} />
      <PeopleBar client={client} />
    </>
  );
  // Sharing is the daemon's owner's (M13), and in sight so it's found (#551).
  const share = panes && session && !state.roles && (
    <button class="k-top-act ws-share" aria-label="Share session" title={offlineWhy(client) ?? "Share this session with someone"} disabled={!client.connected} onClick={() => shareSession(session.id)}>
      Share
    </button>
  );
  return <TopBar steps={steps} views={views} tools={tools} share={share} />;
}
