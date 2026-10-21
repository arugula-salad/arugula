// Arugula service worker: shows push notifications (a pane needs you)
// and opens that pane when one is tapped. An approval (an agent block's
// permission request, or Claude Code's in a terminal, M29) comes with Allow
// and Deny; a question with one or two answers comes with them (M6c); a
// failure or a finished command with Dismiss (M24). Those are answered from
// here, without opening the app: on the daemon's own page with a request to
// it, and through Arugula control over an end-to-end channel the worker
// opens itself, with this device's key and the daemon's Noise key from the
// directory the page checked (S18). A pane that wants you, with room for
// another button, also gets Share, which opens the pane with its session's
// share picker (#237). It also keeps the last copy of the page
// itself, used only when the daemon that serves it doesn't answer: the page
// can then still reach the other hosts on its saved list (M4a). Nothing else
// is cached. Control's own notices (a new device or a team request
// waiting, #104) open its page, which shows the prompt; its chat pushes
// (M123) open the room on the chat page.
//
// Built by itself into dist/sw.js as a classic worker (vite.sw.config.ts).

import { E2ESocket } from "./e2e/channel.ts";
import type { ActRequest } from "../shared/proto";
import { existingKeys, loadWorkerDirectory } from "./e2e/keys.ts";
import { chatTap, shareUrl, tapUrl, withShare } from "./tap.ts";

/** The parts of a service worker's global scope used here (the project's
 * types are the page's). */
interface Worker {
  registration: ServiceWorkerRegistration;
  clients: {
    claim(): Promise<void>;
    matchAll(o?: { type?: string; includeUncontrolled?: boolean }): Promise<(Client & { focus?(): Promise<unknown> })[]>;
    openWindow(url: string): Promise<unknown>;
  };
  location: Location;
  skipWaiting(): Promise<void>;
  addEventListener(type: string, fn: (e: never) => void): void;
}
interface Client {
  postMessage(m: unknown): void;
}
interface Waiting {
  waitUntil(p: Promise<unknown>): void;
}
interface PushEvent extends Waiting {
  data: { json(): unknown; text(): string } | null;
}
interface ClickEvent extends Waiting {
  action: string;
  notification: Notification;
}
interface FetchEvent {
  request: Request;
  respondWith(r: Promise<Response>): void;
}

const sw = self as unknown as Worker;
const SHELL = "arugula-shell-v1";

sw.addEventListener("install", () => void sw.skipWaiting());
sw.addEventListener("activate", (e: Waiting) =>
  e.waitUntil(sw.clients.claim()),
);

sw.addEventListener("fetch", (event: FetchEvent) => {
  const req = event.request;
  const url = new URL(req.url);
  const page = req.mode === "navigate" && url.pathname === "/";
  const asset = url.pathname.startsWith("/assets/") || url.pathname === "/icon.svg";
  if (req.method !== "GET" || url.origin !== sw.location.origin || !(page || asset)) return;
  event.respondWith(
    (async () => {
      const cache = await caches.open(SHELL);
      const key = page ? "/" : req;
      try {
        const res = await fetch(req);
        if (res.ok) await cache.put(key, res.clone());
        return res;
      } catch (e) {
        const saved = await cache.match(key);
        if (saved) return saved;
        throw e;
      }
    })(),
  );
});

interface Msg {
  cloud_run?: string;
  /** A Cloud workspace tab's approval (S40): its workspace and tab. */
  cloud_workspace?: string;
  cloud_tab?: string;
  title?: string;
  body?: string;
  tag?: string;
  pane?: number;
  /** Through control (M21): the daemon it's from. */
  daemon?: string;
  approve?: { id: string; title?: string };
  ask?: { id: string; field: string; options: string[] };
  /** A gate's names it (`member/op/gate`), for Approve and Expire; a
   * decision point's (#621) brings one or two choices to answer with. */
  reason?: { kind: string; actions: string[]; gate?: { id: string; title?: string; choices?: { value: string; label: string }[] } };
  /** Control's own (#104): a device or a person waits for approval. */
  control?: boolean;
  /** M123: control's chat: the room and message a push is about. */
  chat?: { room: string; seq: number };
}

sw.addEventListener("push", (event: PushEvent) => {
  let msg: Msg = { title: "Arugula", body: "" };
  try {
    msg = event.data?.json() as Msg;
  } catch {
    // not JSON: show what came
    if (event.data) msg.body = event.data.text();
  }
  const approve = msg.approve && typeof msg.approve.id === "string" ? msg.approve : null;
  const ask = msg.ask && typeof msg.ask.id === "string" && Array.isArray(msg.ask.options) ? msg.ask : null;
  const reason = msg.reason && Array.isArray(msg.reason.actions) ? msg.reason : null;
  // A gate (M34): Approve, and Expire for a chant gate (#310), else Dismiss.
  const gate = reason && reason.gate && typeof reason.gate.id === "string" && reason.actions.includes("allow") ? reason.gate : null;
  // A decision point's question (#621): a button for each choice.
  const choices = reason?.gate && typeof reason.gate.id === "string" && reason.actions.includes("answer") ? reason.gate.choices : undefined;
  const point = Array.isArray(choices) ? choices : null;
  const offered = approve
    ? [
        { action: "approve", title: "Allow" },
        { action: "deny", title: "Deny" },
      ]
    : ask
      ? ask.options.slice(0, 2).map((o, i) => ({ action: `answer-${i}`, title: o }))
      : point
        ? point.slice(0, 2).map((c, i) => ({ action: `point-${i}`, title: c.label.charAt(0).toUpperCase() + c.label.slice(1) }))
      : gate && reason
        ? [
            { action: "gate-approve", title: "Approve" },
            reason.actions.includes("expire") ? { action: "gate-expire", title: "Expire" } : { action: "dismiss", title: "Dismiss" },
          ]
      : reason && reason.actions.includes("rerun")
        ? [
            { action: "rerun", title: "Rerun" },
            { action: "dismiss", title: "Dismiss" },
          ]
        : reason && reason.actions.includes("dismiss")
          ? [{ action: "dismiss", title: "Dismiss" }]
          : [];
  const actions = withShare(offered, { pane: msg.pane, reason });
  event.waitUntil(
    sw.registration.showNotification(msg.title || "Arugula", {
      body: msg.body || "",
      tag: msg.tag || "arugula",
      renotify: true,
      icon: "/icon.svg",
      requireInteraction: !!(approve || ask || gate || point || msg.cloud_run || msg.cloud_tab),
      actions,
      data: { cloud_run: msg.cloud_run, cloud_workspace: msg.cloud_workspace, cloud_tab: msg.cloud_tab, pane: msg.pane, daemon: msg.daemon, chat: msg.chat, approve, ask, reason, control: msg.control === true },
    } as NotificationOptions),
  );
});

/** `POST /api/attention/act` to the daemon a notification came from: this
 * page's own, or one reached through control over a channel of our own. */
async function act(daemon: string | undefined, body: ActRequest): Promise<boolean> {
  try {
    if (!daemon) {
      const res = await fetch("/api/attention/act", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      return res.ok;
    }
    const [keys, dir] = await Promise.all([existingKeys(), loadWorkerDirectory()]);
    const d = dir.find((x) => x.id === daemon);
    if (!keys || !d) return false;
    const urls = [
      ...d.direct.map((u) => ({ url: `${u.replace(/^http/, "ws").replace(/\/$/, "")}/e2e`, timeoutMs: 1500 })),
      { url: d.relay, timeoutMs: 10_000 },
    ];
    const sock = await E2ESocket.connect(urls, { id: d.id, noise: d.noise }, keys);
    try {
      sock.start();
      const res = await sock.request("POST", "/api/attention/act", body);
      return res.ok;
    } finally {
      sock.close();
    }
  } catch {
    return false;
  }
}

/** Couldn't answer: say so, and let a tap open the pane. */
async function failed(pane: number, daemon: string | undefined, what: string) {
  await sw.registration.showNotification("Couldn't answer that", { body: what, tag: `pane-${pane}`, data: { pane, daemon } });
}

sw.addEventListener("notificationclick", (event: ClickEvent) => {
  event.notification.close();
  const data = (event.notification.data || {}) as Msg & { approve: Msg["approve"] | null; ask: Msg["ask"] | null };
  if (data.cloud_run && /^run_[0-9a-f]+$/.test(data.cloud_run)) {
    const run = data.cloud_run;
    event.waitUntil((async () => {
      const windows = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const window of windows) {
        if (window.focus) { window.postMessage({ type: "open-cloud-run", run }); return window.focus(); }
      }
      return sw.clients.openWindow(`/#cloud=${run}`);
    })());
    return;
  }
  if (data.cloud_workspace && data.cloud_tab && /^cw_[0-9a-f]+$/.test(data.cloud_workspace) && /^cwc_[0-9a-f]+$/.test(data.cloud_tab)) {
    const target = { cloud_workspace: data.cloud_workspace, cloud_tab: data.cloud_tab };
    event.waitUntil((async () => {
      const windows = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const window of windows) {
        if (window.focus) { window.postMessage({ type: "open-cloud-tab", ...target }); return window.focus(); }
      }
      return sw.clients.openWindow(`/#cloud-ws=${target.cloud_workspace}/${target.cloud_tab}`);
    })());
    return;
  }
  const pane = data.pane;
  if ((event.action === "approve" || event.action === "deny") && pane && data.approve) {
    const action = event.action === "approve" ? "allow" : "deny";
    const approve = data.approve;
    event.waitUntil(
      (async () => {
        // Already answered, or the daemon is unreachable: say so.
        if (!(await act(data.daemon, { action, pane, id: approve.id }))) await failed(pane, data.daemon, approve.title || "");
      })(),
    );
    return;
  }
  // A gate (M34): approved, or turned down (#310), by its key, so a gate
  // reached since isn't the one answered.
  const gate = data.reason?.gate;
  if ((event.action === "gate-approve" || event.action === "gate-expire") && pane && gate) {
    const action = event.action === "gate-approve" ? "allow" : "expire";
    event.waitUntil(
      (async () => {
        if (!(await act(data.daemon, { action, pane, id: gate.id }))) await failed(pane, data.daemon, gate.title || "");
      })(),
    );
    return;
  }
  // A decision point's question (#621), answered with the choice picked,
  // by its key, so a question asked since isn't the one answered.
  const chose = /^point-(\d)$/.exec(event.action || "");
  const choice = chose && gate?.choices?.[Number(chose[1])];
  if (choice && pane && gate) {
    event.waitUntil(
      (async () => {
        const body = { action: "answer", pane, id: gate.id, content: { answer: choice.value } } satisfies ActRequest;
        if (!(await act(data.daemon, body))) await failed(pane, data.daemon, gate.title || choice.label);
      })(),
    );
    return;
  }
  const picked = /^answer-(\d)$/.exec(event.action || "");
  if (picked && pane && data.ask) {
    const ask = data.ask;
    const choice = ask.options[Number(picked[1])];
    event.waitUntil(
      (async () => {
        const body = { action: "answer", pane, id: ask.id, content: { [ask.field]: choice } } satisfies ActRequest;
        if (!(await act(data.daemon, body))) await failed(pane, data.daemon, choice);
      })(),
    );
    return;
  }
  // A failed command, typed into its pane again (M11).
  if (event.action === "rerun" && pane) {
    event.waitUntil(
      (async () => {
        if (!(await act(data.daemon, { action: "rerun", pane }))) await failed(pane, data.daemon, "Rerun");
      })(),
    );
    return;
  }
  if (event.action === "dismiss" && pane) {
    event.waitUntil(act(data.daemon, { action: "dismiss", pane }));
    return;
  }
  // Share (#237): the pane, with its session's share picker open.
  if (event.action === "share" && pane) {
    event.waitUntil(
      (async () => {
        const wins = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
        for (const w of wins) {
          if (w.focus) {
            w.postMessage({ type: "open-pane", pane, daemon: data.daemon, share: true });
            return w.focus();
          }
        }
        return sw.clients.openWindow(shareUrl({ pane, daemon: data.daemon }));
      })(),
    );
    return;
  }
  // A chat push from control (M123): the room, at the message.
  const room = data.control ? chatTap(data) : undefined;
  if (room) {
    event.waitUntil(
      (async () => {
        const wins = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
        for (const w of wins) {
          if (w.focus) {
            w.postMessage({ type: "open-room", url: room });
            return w.focus();
          }
        }
        return sw.clients.openWindow(room);
      })(),
    );
    return;
  }
  // Control's notice (#104): its page shows the prompt, with the
  // fingerprint, once it looks again.
  if (data.control) {
    event.waitUntil(
      (async () => {
        const wins = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
        for (const w of wins) {
          if (w.focus) {
            w.postMessage({ type: "control-refresh" });
            return w.focus();
          }
        }
        return sw.clients.openWindow("/");
      })(),
    );
    return;
  }
  // A tap: open the pane (its card shows over it), or the card (tapUrl).
  const url = tapUrl(data);
  event.waitUntil(
    (async () => {
      const wins = await sw.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const w of wins) {
        if (w.focus) {
          w.postMessage({ type: data.reason ? "open-card" : "open-pane", pane, daemon: data.daemon });
          return w.focus();
        }
      }
      return sw.clients.openWindow(url);
    })(),
  );
});
