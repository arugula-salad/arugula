// Standing permission rules (#166): what agent blocks on this daemon allow
// without asking, made by an approval card's "From now on…". They're this
// machine's (in the daemon's state), so the list is the shown host's.
// Opened from the account menu; each can be forgotten here.

import { OPS, opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Modal } from "../../shared/ui/modal";
import type { Client } from "../client";

interface Rule {
  index: number;
  tool: string;
  prefix?: string;
  cwd?: string;
  sprite?: string;
  from?: string;
  at_ms: number;
  text: string;
}

let shown: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openRules(client: Client) {
  shown = client;
  changed();
}

export function RulesLayer() {
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
    <Rules
      client={shown}
      close={() => {
        shown = null;
        changed();
      }}
    />
  );
}

/** `~/x` for `/home/me/x`: the daemon's home, guessed from the path (UI
 * audit #175: full paths ran long). */
const tilde = (p: string) => p.replace(/^(\/home\/[^/]+|\/Users\/[^/]+|\/root)(?=\/|$)/, "~");

/** What it allows, in words: the tool, and the commands it starts with. */
const allows = (r: Rule) => (r.prefix ? `${r.tool} commands starting “${r.prefix}”` : `Any ${r.tool}`);

/** Where: a folder (on a VM), or everywhere. */
const where = (r: Rule) => (r.cwd ? `in ${tilde(r.cwd)}${r.sprite ? ` on ${r.sprite}` : ""}` : "everywhere");

// A Modal like control's dialogs (UI audit #175): Escape, the ×, the
// focus and the title's size are its.
function Rules({ client, close }: { client: Client; close: () => void }) {
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [forgetting, setForgetting] = useState(false);

  const load = async () => {
    try {
      const res = await client.request(OPS["rules.list"].method, opPath("rules.list"));
      const v = await res.json<{ rules?: Rule[]; error?: string }>();
      if (!res.ok) throw new Error(v.error ?? `HTTP ${res.status}`);
      setRules(v.rules ?? []);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => {
    void load();
  }, []);

  const forget = async (index: number | null) => {
    const res = await client.request(OPS["rules.forget_all"].method, index === null ? opPath("rules.forget_all") : opPath("rule.forget", { index })).catch(() => null);
    if (!res?.ok) client.toast("couldn't forget it");
    await load();
  };

  return (
    <>
      <Modal close={close} label="Permission rules" class="rules">
        <h2>Permission rules</h2>
        <p class="hint">
          Agent blocks on this machine allow these without asking. An approval card's <b>From now on…</b> makes one; a block's own{" "}
          <b>Always</b> stays in that block.
        </p>
        {error && <p class="error">{error}</p>}
        {rules && rules.length === 0 && <p class="hint">No standing tool rules yet.</p>}
        <ul class="rules-list">
          {(rules ?? []).map((r) => (
            <li key={`${r.index}-${r.at_ms}`} data-rule={r.index}>
              <span class="rule-text" title={r.from ? `Made when allowing ${r.from}` : r.text}>
                {allows(r)} <span class="dim rule-where">{where(r)}</span>
              </span>
              <button class="secondary small" onClick={() => void forget(r.index)}>
                Forget
              </button>
            </li>
          ))}
        </ul>
        <div class="prompt-buttons">
          {rules && rules.length > 1 && (
            <button type="button" class="secondary danger" onClick={() => setForgetting(true)}>
              Forget all
            </button>
          )}
          <button class="secondary" type="button" onClick={close}>
            Done
          </button>
        </div>
      </Modal>
      {forgetting && (
        <ConfirmRemove
          title={`Forget all ${rules?.length ?? ""} rules?`}
          label="Forget all"
          cancel={() => setForgetting(false)}
          go={() => {
            setForgetting(false);
            void forget(null);
          }}
        >
          <p>Agent blocks on this machine ask again before each of these.</p>
        </ConfirmRemove>
      )}
    </>
  );
}
