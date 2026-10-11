// Standing permission rules (#166): what agent blocks on this daemon allow
// without asking, made by an approval card's "From now on…". They're this
// machine's (in the daemon's state), so the list is the shown host's.
// Opened from the account menu; each can be forgotten here.

import { OPS, opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
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

function Rules({ client, close }: { client: Client; close: () => void }) {
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  useEffect(() => void load(), []);

  const forget = async (index: number | null) => {
    const res = await client.request(OPS["rules.forget_all"].method, index === null ? opPath("rules.forget_all") : opPath("rule.forget", { index })).catch(() => null);
    if (!res?.ok) client.toast("couldn't forget it");
    await load();
  };

  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Permission rules" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt rules" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Permission rules</h2>
        <p class="hint">
          Agent blocks on this machine allow these without asking. An approval card's <b>From now on…</b> makes one; a block's own{" "}
          <b>Always</b> stays in that block.
        </p>
        {error && <p class="error">{error}</p>}
        {rules && rules.length === 0 && <p class="hint">None yet: every agent block asks.</p>}
        <ul class="rules-list">
          {(rules ?? []).map((r) => (
            <li key={`${r.index}-${r.at_ms}`} data-rule={r.index}>
              <span class="rule-text" title={r.from ? `Made when allowing ${r.from}` : undefined}>
                {r.text}
              </span>
              <button onClick={() => void forget(r.index)}>Forget</button>
            </li>
          ))}
        </ul>
        <div class="prompt-buttons">
          {rules && rules.length > 1 && (
            <button type="button" class="danger" onClick={() => void forget(null)}>
              Forget all
            </button>
          )}
          <button type="button" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
