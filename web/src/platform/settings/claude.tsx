// Claude through Arugula (M231), in the account's settings: which registered
// clients (studio's lobby) may act for this account, and what was spent on
// its own key this month, by whom. A client names the account in its calls,
// and spends the account's own key when it names it as the one who pays (a
// sponsor, a visitor), only once the account lets it here; each switch is
// off until then.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { session } from "../session";

interface Client {
  client: string;
  allowed: boolean;
  names_payers: boolean;
}

interface Paid {
  month: string;
  usd: number;
  lines: { project: string; account: string | null; client: string | null; calls: number; usd: number }[];
}

async function call<T>(method: string, path: string): Promise<T> {
  const r = await platformFetch(path, { method, credentials: "same-origin" });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

/** A project's name, and an account's in any project's team, as the
 * session knows them; the id where it doesn't. */
const projectName = (id: string) => session.project(id)?.name ?? id;
function accountName(id: string): string {
  for (const p of session.projects) for (const m of p.team?.members ?? []) if (m.account === id) return m.name;
  return id;
}

const dollars = (usd: number) => `$${usd < 0.01 && usd > 0 ? usd.toFixed(4) : usd.toFixed(2)}`;

export function ClaudeSettings({ me }: { me?: string }) {
  const [clients, setClients] = useState<Client[] | null>(null);
  const [paid, setPaid] = useState<Paid | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    call<Client[]>("GET", "/api/inference/clients").then(setClients, () => setClients([]));
    call<Paid>("GET", "/api/inference/spend/paid").then(setPaid, () => setPaid(null));
  }, []);
  if (!clients?.length && !paid?.lines.length) return null;
  const flip = async (c: Client) => {
    setBusy(true);
    setErr("");
    try {
      setClients(await call<Client[]>(c.allowed ? "DELETE" : "PUT", `/api/inference/clients/${encodeURIComponent(c.client)}/consent`));
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-claude-settings>
      <h2>Claude</h2>
      {clients && clients.length > 0 && (
        <>
          <p class="dim">
            Apps that use Claude through Arugula for you. One you let calls Claude in your name, on your projects' keys or on
            Arugula's key within your monthly cap. One that can name who pays may also spend your own key for people you sponsor or
            visit. None can until you let it, and stopping it takes effect on its next call.
          </p>
          <ul class="platform-list" data-claude-clients>
            {clients.map((c) => (
              <li key={c.client}>
                <label class="platform-mirror">
                  <input type="checkbox" data-claude-client={c.client} checked={c.allowed} disabled={busy} onChange={() => void flip(c)} />{" "}
                  Let <b>{c.client}</b> use Claude for me
                  {c.names_payers ? <span class="dim"> (it can spend my own key for people it names)</span> : null}
                </label>
              </li>
            ))}
          </ul>
        </>
      )}
      {paid && paid.lines.length > 0 && (
        <>
          <p class="dim" data-claude-paid>
            Spent on your own key in {paid.month} (UTC), at Anthropic's list prices: {dollars(paid.usd)}.
          </p>
          <ul class="platform-list">
            {paid.lines.map((l) => (
              <li key={`${l.project}-${l.account}-${l.client}`}>
                {l.account === me ? "You" : l.account ? accountName(l.account) : "a deleted account"}
                {l.client ? <span class="dim"> · through {l.client}</span> : null} · {dollars(l.usd)}
                <span class="dim">
                  {" "}
                  · {l.calls} {l.calls === 1 ? "call" : "calls"} in {projectName(l.project)}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {err && <p class="control-error">{err}</p>}
    </div>
  );
}
