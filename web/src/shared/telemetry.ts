// Usage counts (the telemetry track): which views a person opens and how
// often, and how many errors our code hit. Counts only: never what a view
// showed, an error's message or its stack, a name, a path or a command.
//
// Nothing is counted until a yes turns it on (`startCounting`), with the
// send function for where that yes was given: control's page sends to
// control under the account (M104, control.ts); the machine's web client
// will send through its daemon (M103). Each minute and as the page goes
// away, what was counted goes to that function and starts again from zero.

/** The views counted, as control-wire's `telemetry::View` spells them. */
export type View =
  | "swarm"
  | "chat"
  | "conversations"
  | "agents"
  | "palette"
  | "settings"
  | "getting-started"
  | "share"
  // Every hint's together (shell/hints.ts).
  | "hint-shown"
  | "hint-acted"
  | "hint-dismissed"
  // Every pane modal's together; each counted on its own adds its own
  // (`Hint.views`, shell/hints).
  | "pane-tip-shown"
  | "pane-tip-acted"
  | "pane-tip-closed"
  | "pane-tip-opted-out";

/** What was counted since the last send. */
export interface PageCounts {
  views: Partial<Record<View, number>>;
  web_errors: number;
}

/** Sends one batch. `leaving`: the page is going away, so it has to go
 * now (a `keepalive` fetch or a beacon). Says false to stop counting (the
 * other end said no). */
export type Send = (c: PageCounts, leaving: boolean) => Promise<boolean | void> | boolean | void;

const EVERY_MS = 60_000;

let send: Send | null = null;
let counts: PageCounts = { views: {}, web_errors: 0 };
let stopFns: (() => void)[] = [];

/** Under a test runner (Playwright sets `navigator.webdriver`): never ask
 * and never send, unless a spec says it wants to (`__arugulaTelemetry`). */
export function automated(): boolean {
  return !!navigator.webdriver && !(window as { __arugulaTelemetry?: boolean }).__arugulaTelemetry;
}

export function counting(): boolean {
  return send !== null;
}

/** One more `view` opened. Nothing while counting is off. */
export function countView(v: View) {
  if (!send) return;
  counts.views[v] = (counts.views[v] ?? 0) + 1;
}

/** The view a route in the address (`#swarm`, `#chat…`, `#agents…`) is. */
function routeView(hash: string): View | null {
  if (/^#swarm(=|$)/.test(hash)) return "swarm";
  if (/^#chat(=|$)/.test(hash)) return "chat";
  if (/^#agents(=|$)/.test(hash)) return "agents";
  return null;
}

/** Start counting, sending to `to`; replaces whatever sent before. */
export function startCounting(to: Send) {
  stopCounting();
  if (automated()) return;
  send = to;
  let route = routeView(location.hash);
  if (route) countView(route);
  const onHash = () => {
    const now = routeView(location.hash);
    if (now && now !== route) countView(now);
    route = now;
  };
  // Only errors from our own bundle (not an extension's, nor another
  // site's script), and only how many: the event itself is never read.
  const onError = (e: ErrorEvent) => {
    if (typeof e.filename === "string" && e.filename.startsWith(location.origin)) counts.web_errors++;
  };
  const onRejection = () => void counts.web_errors++;
  const onHide = () => flush(true);
  addEventListener("hashchange", onHash);
  addEventListener("error", onError);
  addEventListener("unhandledrejection", onRejection);
  addEventListener("pagehide", onHide);
  const timer = setInterval(() => flush(false), EVERY_MS);
  stopFns = [
    () => removeEventListener("hashchange", onHash),
    () => removeEventListener("error", onError),
    () => removeEventListener("unhandledrejection", onRejection),
    () => removeEventListener("pagehide", onHide),
    () => clearInterval(timer),
  ];
  // Says it's counting, for a spec to wait for (never what was counted).
  document.documentElement.dataset.usageCounts = "";
}

/** Stop, and drop what was counted but not sent. */
export function stopCounting() {
  for (const f of stopFns) f();
  stopFns = [];
  send = null;
  delete document.documentElement.dataset.usageCounts;
  counts = { views: {}, web_errors: 0 };
}

/** Send what was counted, if anything. A batch that fails is dropped. */
export function flush(leaving: boolean) {
  const to = send;
  if (!to) return;
  const c = counts;
  if (!c.web_errors && !Object.keys(c.views).length) return;
  counts = { views: {}, web_errors: 0 };
  // Called at once, not in a later task: as the page goes away, there's
  // no later.
  let sent: ReturnType<Send>;
  try {
    sent = to(c, leaving);
  } catch {
    return;
  }
  Promise.resolve(sent).then(
      (ok) => {
        if (ok === false && send === to) stopCounting();
      },
      () => {},
    );
}
