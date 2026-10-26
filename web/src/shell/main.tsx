import { AgentDialogLayer, startAgent } from "./ui/agent-dialog";
import { chat } from "../shared/chat-store";
import { shellPlatformApi } from "./platform-api";
import { platformOrigin, setPlatformOrigin } from "./platform-links";
import { platformPath } from "./fragments";
import { PlatformApiContext } from "../shared/platform-api";
// Arugula web client: tabs and splits of terminals owned by the daemon.

import { OPS, opPath } from "./ops";
import { setLending } from "./hand";
import { type TeamPins, type TeamPinsRequest } from "../shared/proto";
import { render } from "preact";
import "../shared/fonts.css";
import "../shared/style.css";
import "../shared/frame.css";
import "./shell-chrome.css";
import "./pane-chrome.css";
import "./interaction-chrome.css";
import "./block-chrome.css";
import "./phone-sheet.css";
import "./phone-machines.css";
import { apply as applyAppearance } from "../shared/appearance";
import { AppearanceLayer } from "../shared/ui/appearance";
import { Client } from "./client";
import { Fleet, type HostRef } from "./fleet";
import { directory } from "./hosts";
import { App } from "./ui/app";
import { openTeamChat, remotes } from "./blocks";
import { measureCell } from "./ui/cells";
import { desktopPush, enableControlPush, registerWorker, setPushBackend } from "./push";
import { ControlSession, detectControl, restoreInvite, showMoving } from "./control";
import { useControl } from "./trust/common";
import { ControlGate } from "./trust/gate";
import { NoMachines } from "./trust/machines";
import { accountItems, ControlOverlay, controlMenuItems } from "./trust/overlay";
import { setFleet, setHostMenuExtras } from "./ui/hosts";
import { setControlSession, shareSession } from "./ui/people";
import { unhex } from "./e2e/cert.ts";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { useEffect, useState } from "preact/hooks";
import { SwarmView } from "./swarm/view";
import { AppBar, setAccount } from "./ui/app-bar";
import { fakeSwarm } from "./swarm/fake";
import { closeSwarm, onSwarmRoute, swarmRoute } from "./swarm/route";
import { setupDesktop, tauriInvoke } from "../shared/desktop";
import { installKeys } from "./ui/keys";
import { openDeveloper } from "./ui/developer";
import { openGettingStarted, type Section } from "./ui/welcome";
import { followMachine, machineTelemetryChanged, MachineTelemetryLayer } from "./ui/machine-telemetry";

// Served by Arugula control (M17), not a daemon: sign in, enroll this
// browser, and reach daemons through end-to-end channels. A read-only link
// (M19, `#link=…`) needs no account: its key is in the fragment.
// This device's palette and finish, before anything draws.
applyAppearance();
const info = await detectControl();
// Back from signing in with a presigned invite's link (kept off control).
if (info) restoreInvite();
if (info) showMoving(info);
// M153: chat is on the platform page, when control has one.
setPlatformOrigin(info?.platform);
// M154: an old link of the platform's (`#chat…`, `#cloud…`, `#cloud-ws…`)
// lands on its new page there. Keep this forever (./fragments).
const moved = platformOrigin() ? platformPath(location.hash) : null;
if (moved) {
  location.replace(`${platformOrigin()}${moved}`);
  await new Promise(() => {});
}
// …and one followed on this page, as a hash change.
addEventListener("hashchange", () => {
  const to = platformOrigin() ? platformPath(location.hash) : null;
  if (to) location.replace(`${platformOrigin()}${to}`);
});
const linkMatch = /^#link=([0-9a-f]+)\.([0-9a-f]{64})\.([0-9a-f]{64})\.([0-9a-f]{64})$/.exec(location.hash);
const session = info && !linkMatch ? new ControlSession(info) : null;
setControlSession(session);
let linkTarget: import("./client").E2ETarget | undefined;
if (info && linkMatch) {
  const [, daemon, noise, seed, pub] = linkMatch;
  const pkcs8 = new Uint8Array(48);
  pkcs8.set(unhex("302e020100300506032b656e04220420"));
  pkcs8.set(unhex(seed), 16);
  const privateKey = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "X25519" }, false, ["deriveBits"]);
  const publicKey = await crypto.subtle.importKey("raw", unhex(pub), { name: "X25519" }, true, []);
  const keys = { noise: { privateKey, publicKey }, sign: undefined as unknown as CryptoKeyPair, id: "link", noisePub: pub, signPub: "" };
  linkTarget = { daemon: { id: daemon, noise }, direct: [], relay: `${info.url.replace(/^http/, "ws")}/api/relay/link/${daemon}`, keys };
  // Keep the key out of the address bar (and of anything that reads it).
  history.replaceState(null, "", "/");
}
// S33: lend this device's tools to agents, if it was turned on.
setLending(session);
if (session) {
  setHostMenuExtras(() => controlMenuItems(session));
  setAccount({
    login: () => session.login || null,
    items: () => accountItems(session, client.flag("docs")),
    signOut: () => void session.signOut(false),
    subscribe: (fn) => session.subscribe(fn),
  });
  // A tapped notice from control (#104): what waits shows now.
  addEventListener("arugula:control-refresh", () => void session.refresh());
  setPushBackend({
    enable: () => enableControlPush(session.info.vapid, (sub) => session.subscribePush(sub)),
    disable: async () => {
      const reg = await navigator.serviceWorker.getRegistration();
      const sub = await reg?.pushManager.getSubscription();
      if (sub) {
        await session.unsubscribePush(sub.endpoint).catch(() => {});
        await sub.unsubscribe();
      }
      return "off";
    },
    prefs: () => session.pushPrefs(),
    setPrefs: (p) => session.setPushPrefs(p),
  });
  // The desktop app has no Web Push: it shows what control pushes itself,
  // while it runs, once it says it can (an app from before doesn't).
  const invoke = tauriInvoke();
  if (invoke)
    void invoke("notices_state").then(
      () =>
        setPushBackend(
          desktopPush(invoke, {
            subscribe: (sub) => session.subscribePush(sub),
            unsubscribe: (endpoint) => session.unsubscribePush(endpoint),
            prefs: () => session.pushPrefs(),
            setPrefs: (p) => session.setPushPrefs(p),
          }),
        ),
      () => {},
    );
  session.subscribe(() => {
    directory.setControl(
      session.daemons.map((d) => ({
        name: d.name,
        id: d.id,
        urls: d.urls,
        transport: "control" as const,
        added_ms: 0,
        last_seen_ms: d.last_seen,
        status: d.online ? "online" : "offline",
      })),
      session.stale,
    );
    // M123: chat is control's, once the account is in (it asks whether
    // control's `chat` flag is on for it).
    if (session.phase === "ready" && session.account) {
      const roster = session.teams.flatMap((t) => t.roster.members.filter((m) => m.account === session.account).map((m) => m.name));
      chat.start(session.account, [session.login, session.name, ...roster]);
    }
  });
  void session.boot();
}

function makeClient(base: string): Client {
  if (linkTarget) return new Client(`e2e:${linkTarget.daemon.id}`, linkTarget);
  if (session && base.startsWith("e2e:")) return new Client(base, session.target(base.slice(4)));
  return new Client(base);
}

// One client for the host shown (M4a); switching hosts closes it and opens
// one to the other daemon, so hidden hosts hold no connection.
let client = makeClient(directory.base());
const swapped = new Set<() => void>();
/** Call `done` with the shown machine's client, again on each change it
 * announces and on each swap to another, until it returns true. What the
 * page opens from its address on load needs this: on control's page the
 * client at first is a stand-in on control's own origin, there until the
 * machine list comes and `follow` swaps in the machine's (relayed) one. A
 * request through the stand-in reaches control, not the machine. */
function untilShown(done: (c: Client) => boolean) {
  let on: Client | null = null;
  let off = () => {};
  const check = () => {
    if (done(client)) {
      off();
      swapped.delete(check);
      return;
    }
    // Once per client: re-adding a listener while it emits would revisit it.
    if (on === client) return;
    off();
    on = client;
    off = client.subscribe(check);
  };
  swapped.add(check);
  check();
}
// Inside the desktop app: its titlebar (M46). The page's keys, the app's
// among them (ui/keys.ts).
setupDesktop();
installKeys(() => client);
/** Hand a machine of yours the teams this browser pinned (#233), and drop
 * those it has that this account isn't in now (only once the teams have
 * loaded: a failed load isn't "none"). */
async function syncPins(c: Client, session: ControlSession) {
  const pins = session.teamPins();
  let drop: string[] = [];
  if (session.teamsLoaded) {
    const r = await c.request(OPS["team_pins.get"].method, opPath("team_pins.get"));
    const had = r.ok ? ((await r.json<TeamPins>()).pins ?? {}) : {};
    const mine = new Set(session.teams.map((t) => t.team));
    drop = Object.keys(had).filter((t) => !mine.has(t));
  }
  if (!Object.keys(pins).length && !drop.length) return;
  const r = await c.request(OPS["team_pins.set"].method, opPath("team_pins.set"), { pins, drop } satisfies TeamPinsRequest);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
}

const connect = () => {
  // M103: the shown machine's usage reports, counted only on a page that
  // talks to its daemon directly (control's page counts its own, M104).
  // Asked there, and on control's page once the account's question is out
  // of the way; never on a read-only link.
  followMachine(client, !info && !linkTarget, () => !linkTarget && (!session || session.telemetryAsked()));
  // In control mode there's nothing to connect to until a daemon is known.
  if (!session || client.e2e) client.connect();
  if (session) {
    const c = client;
    directory.setPath(null);
    let pinned = false;
    c.subscribe(() => {
      if (c !== client) return;
      directory.setPath(c.connected ? c.path : null);
      // A machine of yours learns the teams this browser pinned (#233), so
      // their members can be invited there, and forgets those this account
      // left; it checks each roster itself.
      const id = c.e2e?.daemon.id;
      if (c.connected && c.state && !c.state.roles && !pinned && id && session.owns(id)) {
        pinned = true;
        void syncPins(c, session).catch(() => (pinned = false));
      }
    });
  }
};

// The visible height excludes a phone's on-screen keyboard, so the key bar
// sits just above it.
const vv = window.visualViewport;
const fitViewport = () => document.documentElement.style.setProperty("--app-height", `${vv?.height ?? innerHeight}px`);
vv?.addEventListener("resize", fitViewport);
fitViewport();

const cell = await measureCell();
const root = document.getElementById("root")!;
function ControlRoot({ s }: { s: ControlSession }) {
  useControl(s);
  useSubscribe((fn) => directory.subscribe(fn));
  const body =
    s.phase !== "ready" ? (
      <ControlGate s={s} />
    ) : client.e2e ? (
      <App key={client.base} client={client} cell={cell} />
    ) : s.daemons.length ? (
      // Machines, but none picked yet: not "Add a machine" (#749).
      <Connecting name={directory.current} />
    ) : (
      <NoMachines s={s} />
    );
  return (
    <>
      {body}
      <ControlOverlay s={s} />
    </>
  );
}

/** Between knowing the account's machines and showing one. */
function Connecting({ name }: { name: string }) {
  return (
    <div class="empty" data-connecting>
      <p>Connecting to {name || "your machine"}…</p>
    </div>
  );
}

// Every host at once (M25): a summaries-only connection to each, for the
// swarm. The tab view above still connects for real to the one it shows.
const fleet = new Fleet((h) => {
  if (session) {
    const t = h.id ? session.target(h.id) : undefined;
    return t ? new Client(`e2e:${h.id}`, t, true) : null;
  }
  return new Client(directory.base(h.name), undefined, true);
});
const fleetHosts = (): HostRef[] =>
  directory.names.map((name) => {
    const h = directory.find(name);
    const d = session?.daemons.find((x) => x.id === h?.id);
    return {
      name,
      id: h?.id,
      transport: name === directory.home ? "home" : (h?.transport ?? "tailnet"),
      status: h?.status,
      owner: d?.account ? (d.owner_name ?? d.account) : undefined,
      team: d?.team ?? null,
      ownerId: d?.account,
      teamName: d?.team ? session?.teams.find((t) => t.team === d.team)?.roster.name : undefined,
      hosted: !!d?.hosted,
    };
  });
fleet.onOpen = (host, pane, share, reveal) => {
  directory.select(host);
  // The host's client is made when the directory switches, then connects:
  // show the pane once it knows it.
  const until = Date.now() + 15_000;
  const go = () => {
    if (client.base === directory.base(host) && client.info(pane)) {
      client.setActive(pane);
      if (share) shareFrom(pane);
      // A chat quote (M123): its lines in the pane, once it has drawn.
      if (reveal) requestAnimationFrame(() => client.panes.get(pane)?.view.reveal(reveal));
      return;
    }
    if (Date.now() < until) setTimeout(go, 100);
  };
  go();
};
/** #237: the share picker for a pane's session, from a card or a push,
 * once the pane is shown. Only its owner shares (as in the session menu);
 * anyone else just gets the pane. */
const shareFrom = (pane: number) => {
  const s = client.sessionOfPane(pane);
  if (s !== null && !client.state?.roles) shareSession(s);
};
setFleet(fleet);
if (!linkTarget) {
  // Teams load after the directory: their names come with them (M30).
  session?.subscribe(() => {
    machineTelemetryChanged();
    if (session.login) fleet.me = session.login;
    fleet.setHosts(fleetHosts());
  });
  directory.subscribe(() => fleet.setHosts(fleetHosts()));
  fleet.setHosts(fleetHosts());
  fleet.start();
}

/** The swarm (M26) over the tab view, when `/#swarm` says so. */
function Swarm({ client }: { client: Client }) {
  const [, set] = useState(0);
  useEffect(() => onSwarmRoute(() => set((n) => n + 1)), []);
  // The home machine's name arrives with the directory, after a cold load.
  useSubscribe((fn) => directory.subscribe(fn));
  // The shared bar shows once the shown machine has answered.
  useSubscribe((fn) => client.subscribe(fn));
  const phone = usePhone();
  const route = swarmRoute();
  if (!route || linkTarget) return null;
  if (session && session.phase !== "ready") return null;
  const f = route.focus;
  // A notification through control names its daemon; else it's this page's.
  const host = f ? (f.daemon ? directory.list?.hosts.find((h) => h.id === f.daemon)?.name : (directory.home ?? directory.names[0])) : undefined;
  const home = directory.home ?? directory.names[0] ?? null;
  const view = (bar: boolean) => <SwarmView fleet={fleet} back={closeSwarm} home={home} focus={f && host ? { host, pane: f.pane } : null} inBar={bar} />;
  // The bar every place shares, over it, once a machine is shown; a phone
  // has its own header, and the swarm its Panes button.
  if (!client.state || phone) return view(false);
  return (
    <div class="swarm-shell">
      <AppBar client={client} at="swarm" all />
      {view(true)}
    </div>
  );
}

const draw = () =>
  render(
    // What a pane's thread reaches the shell through (M147).
    <PlatformApiContext.Provider value={shellPlatformApi(client)}>
      {session ? <ControlRoot key={client.base} s={session} /> : <App key={client.base} client={client} cell={cell} />}
      <Swarm key={client.base} client={client} />
      <AgentDialogLayer />
      <AppearanceLayer />
      <MachineTelemetryLayer />
    </PlatformApiContext.Provider>,
    root,
  );
draw();
connect();

// A link's page shows its one daemon: no host list.
if (!linkTarget) {
  const follow = () => {
    const base = directory.base();
    if (base === client.base) return;
    client.close();
    client = makeClient(base);
    draw();
    connect();
    for (const check of [...swapped]) check();
  };
  directory.subscribe(follow);
  // Control's machine list may have come while this module waited on the
  // fonts above (#749): its change was announced before anyone listened,
  // and the page would show "Add a machine" until the next refresh.
  follow();
  void directory.refresh();
}


// Opened from a notification (`#pane=N`), or told to by the service
// worker. Notifications come from the home daemon, so show it first.
const openPane = (pane: number, daemon?: string, share = false, follow?: string) => {
  // From a notification through control: that daemon's host first.
  const host = daemon ? directory.list?.hosts.find((h) => h.id === daemon)?.name : undefined;
  if (host) directory.select(host);
  else if (directory.home !== null) directory.select(directory.home);
  let watched = false;
  const go = () => {
    if (!client.info(pane)) return false;
    client.setActive(pane);
    if (share) shareFrom(pane);
    // Once: watching emits, and this runs again from inside that.
    if (follow && !watched) {
      watched = true;
      client.watch(follow, pane);
    }
    return true;
  };
  untilShown(go);
};
// The desktop app up to 0.26.3 names its events `arugula:…` only when the
// page says it's from after the rename (#505); without this they arrive as
// `illogical:…`, which nothing here hears (#708). Newer apps always send
// `arugula:…`. Keep it until no 0.26 app is in use.
(window as { __arugulaPage?: boolean }).__arugulaPage = true;
// A pane opened on the home daemon from elsewhere (a sandbox shell).
window.addEventListener("arugula:open-pane", (e) => openPane((e as CustomEvent<number>).detail));
// A click on what control pushed, in the desktop app, with control's page
// already open (crates/desktop/src/notices.rs): as the service worker's
// messages do (push.ts).
window.addEventListener("arugula:notice", (e) => {
  const d = ((e as CustomEvent).detail ?? {}) as { pane?: unknown; daemon?: unknown; card?: unknown; hash?: unknown; refresh?: unknown };
  const daemon = typeof d.daemon === "string" && /^[0-9a-f]+$/.test(d.daemon) ? d.daemon : undefined;
  if (d.refresh === true) dispatchEvent(new Event("arugula:control-refresh"));
  else if (typeof d.hash === "string" && /^chat=[a-z]{2}_[0-9a-f]+(?:&msg=-?\d+)?$/.test(d.hash)) location.hash = d.hash;
  else if (typeof d.pane === "number" && d.card === true) location.hash = `swarm=${daemon ? `${daemon}.` : ""}${d.pane}`;
  else if (typeof d.pane === "number") openPane(d.pane, daemon);
});
// A push's Share (#237) adds `&share`: the share picker opens over it.
// An older daemon's mention push (before M125) adds `&thread=…`: the pane
// opens, and its room is in chat.
const fromHash = /^#pane=(?:([0-9a-f]+)\.)?(\d+)(?:&thread=(?:pane|session)-\d+)?(&share)?(?:&follow=([^&]+))?$/.exec(location.hash);
if (fromHash) {
  const [, daemon, pane, share, follow] = fromHash;
  // A notification through control names its daemon: once its host is in
  // the list, open the pane there.
  const go = () => openPane(Number(pane), daemon, !!share, follow && decodeURIComponent(follow));
  if (daemon && !directory.find?.(directory.list?.hosts.find((h) => h.id === daemon)?.name ?? "")) {
    const off = directory.subscribe(() => {
      if (directory.list?.hosts.some((h) => h.id === daemon)) {
        off();
        go();
      }
    });
  } else go();
  history.replaceState(null, "", "/");
}
if (!linkTarget) void registerWorker(openPane);

// M184: the platform page's hosted workspace opens its machine here
// (`#machine=<id>`), and its + Agent Start an agent on it (`&agent`), once
// the machine is in the list.
const toMachine = /^#machine=([0-9A-Za-z_-]+)(&agent)?$/.exec(location.hash);
if (toMachine) {
  const [, id, agent] = toMachine;
  const go = () => {
    const host = directory.list?.hosts.find((h) => h.id === id)?.name;
    if (!host) return false;
    directory.select(host);
    if (agent)
      untilShown((c) => {
        if (!c.connected || directory.current !== host) return false;
        startAgent(c, {});
        return true;
      });
    return true;
  };
  if (!go()) {
    const off = directory.subscribe(() => {
      if (go()) off();
    });
  }
  history.replaceState(null, "", "/");
}

// A room's link to the multiplexer side (#123): `#chat-block` is team chat in
// a block here, `#chat-block=<room>` (`&msg=N`) on that room. The platform
// page's own `#chat…` links still go to its chat page.
const chatBlockAtHash = () => {
  const m = /^#chat-block(?:=([a-z]{2,3}_[0-9a-f]{4,64})(?:&msg=(-?\d+))?)?$/.exec(location.hash);
  if (!m) return;
  history.replaceState(null, "", location.pathname + location.search);
  untilShown((c) => {
    if (!c.connected || !c.state) return false;
    void openTeamChat(c, { room: m[1], msg: m[2] !== undefined ? Number(m[2]) : undefined, session: c.session ?? undefined });
    return true;
  });
};
chatBlockAtHash();
window.addEventListener("hashchange", chatBlockAtHash);

// The desktop app's notification when control drops this machine, and its
// Daemon menu (#325): Join… and Join again… open Getting started at the
// cloud step, on an open page (the event) or a new one
// (`#getting-started=cloud`).
window.addEventListener("arugula:getting-started", (e) =>
  openGettingStarted((e as CustomEvent<Section>).detail, client),
);
const startAt = /^#getting-started=(\w+)$/.exec(location.hash);
if (startAt) {
  openGettingStarted(startAt[1] as Section, client);
  history.replaceState(null, "", "/");
}

// Developer settings (#665), from the desktop app's Daemon menu: on an open
// page (the event) or a new one (`#developer`). The owner's; the dialog
// says so for anyone else, since the daemon refuses them.
// The flags are the machine's: open them once it's connected, not through
// control's page's stand-in client while the machine list loads.
const developer = () =>
  untilShown((c) => {
    if (!c.connected) return false;
    openDeveloper(c);
    return true;
  });
window.addEventListener("arugula:developer", developer);
const developerAtHash = () => {
  if (location.hash !== "#developer") return;
  developer();
  history.replaceState(null, "", "/");
};
developerAtHash();
window.addEventListener("hashchange", developerAtHash);

// For end-to-end tests.
Object.assign(window, {
  __arugula: {
    get client() {
      return client;
    },
    hosts: directory,
    /** #17: the connections remote panes use, by host. */
    remotes,
    control: session,
    /** M123: control's chat (rooms, messages, the live stream). */
    chat,
    fleet,
    /** M26: made-up panes in the swarm (frame-rate check, screenshots). */
    swarmFake: (n: number) => fakeSwarm(fleet, n),
    /** M26: the swarm's field, when it's shown. */
    get swarm() {
      return (window as unknown as { __swarm?: unknown }).__swarm ?? null;
    },
    /** M23: a second connection to the same daemon that only takes
     * summaries (what the swarm and the fleet use). */
    summaries: () => {
      const c = new Client(client.base, client.e2e, true);
      c.connect();
      return c;
    },
    cell,
    text: (pane: number) => client.panes.get(pane)?.view.text() ?? client.blocks.get(pane)?.view.text() ?? "",
    screen: (pane: number) => client.panes.get(pane)?.view.screen() ?? "",
    size: (pane: number) => {
      const v = client.panes.get(pane)?.view;
      return v ? [v.cols, v.rows] : null;
    },
    offset: (pane: number) => client.panes.get(pane)?.offset ?? null,
    /** #1049: whether a terminal draws with WebGL now. */
    webgl: (pane: number) => client.panes.get(pane)?.view.webglOn ?? null,
    selection: (pane: number) => client.panes.get(pane)?.view.selection() ?? "",
  },
});
