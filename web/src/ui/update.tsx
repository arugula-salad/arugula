// A newer release is out (#176): a small chip in the top bar, and the
// command that updates this install. The daemon checks, at most twice a
// day (`GET /api/update`, crates/daemon/src/update.rs). In the desktop app,
// when it updates itself, *Update now* does it all: the app puts the new
// one in place and restarts, and the new app updates the daemon
// (crates/desktop/src/updates.rs).

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { CopyText } from "./copy";

/** `GET /api/update`. */
export type UpdateStatus = {
  current: string;
  latest?: string;
  newer: boolean;
  enabled: boolean;
  kind: "script" | "brew" | "app" | "source";
  command?: string;
  url: string;
};

const DISMISS_KEY = "illogical.update.dismissed";
/** Ask the daemon again this often (it checks GitHub far less). */
const POLL_MS = 60 * 60 * 1000;
const SOON_MS = 30 * 1000;

function dismissed(): string | null {
  try {
    return localStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

/** The desktop app's window says so (crates/desktop/src/cloud.rs). */
function inApp(): boolean {
  return !!(window as { __illogicalApp?: unknown }).__illogicalApp;
}

/** The app updates itself and the daemon (`window.__illogicalApp.updates`). */
function appUpdates(): boolean {
  return !!(window as { __illogicalApp?: { updates?: boolean } }).__illogicalApp?.updates;
}

type Invoke = <T>(cmd: string) => Promise<T>;

/** A command of the app's (crates/desktop/src/updates.rs). */
function app<T>(cmd: string): Promise<T> {
  const invoke = (globalThis as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__?.core?.invoke;
  if (!invoke) return Promise.reject(new Error("the app isn't reachable from this page"));
  return invoke<T>(cmd);
}

export function UpdateChip({ client }: { client: Client }) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [gone, setGone] = useState(dismissed);
  // The owner's own daemon only: not a guest, not a page from control. In
  // an app that updates itself, this machine's, whatever page it shows
  // (control's too, when joined): the app asks the daemon.
  const viaApp = appUpdates();
  const mine = viaApp || (!client.e2e && !client.state?.roles);
  useEffect(() => {
    if (!mine) return;
    let t: number | undefined;
    let live = true;
    const get = async () => {
      const s = await (viaApp
        ? app<UpdateStatus>("app_update_status")
        : fetch("/api/update").then((r) => (r.ok ? (r.json() as Promise<UpdateStatus>) : null))
      ).catch(() => null);
      if (!live) return;
      setStatus(s);
      // A daemon that just started checks in a few seconds: look again soon.
      t = window.setTimeout(get, s?.enabled && !s.latest ? SOON_MS : POLL_MS);
    };
    void get();
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [mine, viaApp]);
  if (!mine || !status?.newer || !status.latest || gone === status.latest) return null;
  const latest = status.latest;
  const dismiss = () => {
    try {
      localStorage.setItem(DISMISS_KEY, latest);
    } catch {
      // Shown again next time; harmless.
    }
    setGone(latest);
  };
  return (
    <div class="update">
      <button class="update-chip" title={`illogical ${latest} is out (this is ${status.current})`} data-update-chip onClick={() => setOpen(!open)}>
        Update {latest}
      </button>
      {open && (
        <div class="update-pop" role="dialog" aria-label="Update illogical" data-update>
          <p>
            <b>illogical {latest}</b> is out; {viaApp ? "this machine's daemon" : "this daemon"} is {status.current}. Panes keep running while it restarts.
          </p>
          <How status={status} />
          <div class="update-actions">
            <a href={status.url} target="_blank" rel="noreferrer">
              What's new
            </a>
            <button onClick={dismiss} data-update-dismiss>
              Not now
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function How({ status }: { status: UpdateStatus }) {
  if (appUpdates()) return <UpdateNow />;
  if (inApp() || status.kind === "app")
    return (
      <p>
        Download the new app from the release page and open it: it updates the daemon itself.
        {status.command && (
          <>
            {" "}
            Or, in a terminal: <CopyText text={status.command} data-update-command />
          </>
        )}
      </p>
    );
  if (status.command)
    return (
      <p>
        Run this in a terminal on this machine:
        <CopyText text={status.command} data-update-command />
      </p>
    );
  return <p>Build it from the new release's source, then run <code>illogicald install</code> again.</p>;
}

/** The app updates itself, then the daemon. */
function UpdateNow() {
  const [state, setState] = useState<{ busy?: boolean; note?: string }>({});
  const go = () => {
    setState({ busy: true });
    // Restarting closes this page; anything else comes back.
    app<"daemon" | "current">("app_update").then(
      (r) =>
        setState({
          note: r === "current" ? "This app is already the newest it can find. The release may still be publishing: try again in a few minutes." : undefined,
        }),
      (e: unknown) => setState({ note: `Couldn't update: ${e instanceof Error ? e.message : String(e)}` }),
    );
  };
  return (
    <>
      <p>The app downloads the new version and restarts into it, then updates the daemon.</p>
      {state.busy && <p data-update-busy>Downloading… the app restarts when it's ready.</p>}
      {state.note && <p data-update-note>{state.note}</p>}
      <p>
        <button class="update-now" onClick={go} disabled={state.busy} data-update-now>
          Update now
        </button>
      </p>
    </>
  );
}
