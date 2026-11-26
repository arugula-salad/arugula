// The status pill: the phone's connection, errors everywhere, and a hosted
// machine's box waking (M182). The footer it sat beside is the frame's
// (shared/kit/frame/footer.tsx, Lift 3).

import { useState } from "preact/hooks";
import type { Client } from "../../client";
import { api } from "../../control";
import { platformFetch } from "../../../shared/maintenance";

export function StatusPill({ client, errorsOnly }: { client: Client; errorsOnly?: boolean }) {
  if (client.box && (!client.connected || client.box.state === "awake")) return <WakePill client={client} />;
  const text = client.error ?? (client.connected || errorsOnly ? null : client.state ? "reconnecting…" : "connecting…");
  if (!text) return null;
  return (
    <div id="status" role="status" class={client.error ? "error" : ""}>
      {text}
    </div>
  );
}

/** M182: a hosted machine's box as control says it: waking (until its
 * daemon dials in again and the page goes on where it was, then a moment
 * of how long that took), asleep (Wake wakes it), or why it didn't wake,
 * with Rebuild (its box made again from its spec, so asked twice) and Try
 * again. */
function WakePill({ client }: { client: Client }) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const box = client.box!;
  if (box.state === "awake") {
    return (
      <div id="status" role="status" class="wake" data-wake="awake" data-wake-ms={box.ms}>
        awake · {((box.ms ?? 0) / 1000).toFixed(1)} s
      </div>
    );
  }
  if (box.state === "waking") {
    return (
      <div id="status" role="status" class="wake" data-wake="waking">
        waking…
      </div>
    );
  }
  if (box.state === "asleep") {
    return (
      <div id="status" role="status" class="wake" data-wake="asleep">
        Asleep{" "}
        <button class="link" onClick={() => client.wake()} data-wake-again>
          Wake
        </button>
      </div>
    );
  }
  const hosted = client.e2e?.hosted;
  const rebuild = async () => {
    if (!hosted) return;
    if (!armed) return setArmed(true);
    setBusy(true);
    try {
      await api(`/api/projects/${hosted.project}/workspaces/${hosted.workspace}/rebuild`, {}, platformFetch);
      client.toast("Rebuilding: the workspace opens again once its new box is ready");
    } catch (e) {
      client.toast((e as Error).message);
    }
    setBusy(false);
    setArmed(false);
  };
  return (
    <div id="status" role="alert" class="wake error" data-wake="failed" data-wake-code={box.code}>
      {box.why ?? "It didn't wake."}{" "}
      {hosted && box.code !== "starting" && (
        <button class="link" disabled={busy} onClick={() => void rebuild()} data-wake-rebuild
          title="Delete its box and make it again from the workspace's spec">
          {armed ? "Rebuild: its box starts over" : "Rebuild"}
        </button>
      )}{" "}
      <button class="link" onClick={() => client.wake()} data-wake-again>
        Try again
      </button>
    </div>
  );
}
