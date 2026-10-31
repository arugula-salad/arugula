// The machine's sharing settings (native #205): Guest SSH access, which
// was the `guest-ssh` Labs flag, and (#394) whether agents hear their mentions. Its owner's, kept by the daemon
// (`sharing.get`/`sharing.set`, `sharing.json` in its state directory), off
// until they turn it on; control has no say in it. *Guest SSH access…* in
// the machine's settings (the machine menu, or the account menu's *This
// machine*, and the phone's sheet) opens it. With it on, a pane's menu has
// *Invite over ssh…*.

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { Sharing, SharingSetRequest } from "../../shared/proto.gen";

let shown: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openSharing(client: Client) {
  shown = client;
  changed();
}

export function SharingLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!shown) return null;
  return (
    <SharingDialog
      client={shown}
      close={() => {
        shown = null;
        changed();
      }}
    />
  );
}

// A Modal like Developer settings, one switch (UI audit #175's pattern).
function SharingDialog({ client, close }: { client: Client; close: () => void }) {
  const [sharing, setSharing] = useState<Sharing | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    try {
      const res = await client.request(OPS["sharing.get"].method, opPath("sharing.get"));
      const v = await res.json<Sharing | { error?: string }>().catch(() => null);
      if (res.status === 404) throw new Error("This machine's daemon is too old for this setting: update it");
      if (v === null) throw new Error(`This machine's daemon didn't answer (${res.status}): reopen this once it's connected`);
      if (!res.ok) throw new Error((v as { error?: string }).error ?? `couldn't read the setting (${res.status})`);
      setSharing(v as Sharing);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  const set = async (change: SharingSetRequest) => {
    const res = await client
      .request(OPS["sharing.set"].method, opPath("sharing.set"), change satisfies SharingSetRequest)
      .catch(() => null);
    if (!res?.ok) setError("couldn't change it");
    // The pane menu follows the host's features: no reload.
    await Promise.all([load(), client.loadFeatures()]);
  };

  return (
    <Modal close={close} label="Guest SSH access" class="rules sharing">
      <h2>Guest SSH access</h2>
      <p class="hint">
        Lets you invite someone who has only OpenSSH to watch one of this machine's panes, or type in it if you say so: <i>Invite over
        ssh…</i> in the pane's menu, or <code>arugula share --guest</code>. Each invite ends when it expires or you revoke it. Off
        until you, the owner, turn it on here; control can't.
      </p>
      {error && <p class="error">{error}</p>}
      {sharing?.built === false && <p class="hint">This daemon was built without guest ssh: the switch turns nothing on.</p>}
      {sharing && (
        <label class="sharing-switch">
          <input
            type="checkbox"
            role="switch"
            data-sharing-guest-ssh
            checked={sharing.guest_ssh}
            onChange={(e) => void set({ guest_ssh: e.currentTarget.checked })}
          />{" "}
          Guest SSH access on this machine
        </label>
      )}
      <p class="hint">
        A teammate's <code>@</code>-mention of this machine in a thread, or a reply to one of its agents, is handed to the agent's
        session as a message from a teammate (never as your approval). On unless you turn it off here.
      </p>
      {sharing && (
        <label class="sharing-switch">
          <input
            type="checkbox"
            role="switch"
            data-sharing-agent-mentions
            checked={sharing.agent_mentions ?? true}
            onChange={(e) => void set({ agent_mentions: e.currentTarget.checked })}
          />{" "}
          Agents hear their mentions on this machine
        </label>
      )}
      <div class="prompt-buttons">
        <button class="secondary" type="button" onClick={close}>
          Done
        </button>
      </div>
    </Modal>
  );
}
