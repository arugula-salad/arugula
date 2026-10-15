// Run on Fountain (#942, F4): a recipe of this machine's run in a Fountain
// sandbox, as its Fountain agent `recipe-<name>` (upserted before each
// launch). The dialog shows the daemon's preflight first: the checkout the
// sandbox clones (uncommitted work here doesn't travel), what doesn't carry
// over (a skill's other files, a tool allowlist, a server with a value typed
// in), and the variables the agent uses that Fountain lacks, each with an
// explicit "Copy from this machine to vault X" (never automatic, and the
// value is never shown). The agents block and the Agents page both open it,
// each with its own way to reach the daemon (`FountainApi`).

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";

interface LeftOut {
  name: string;
  why: string;
}
interface Preflight {
  recipe: string;
  agent: string;
  hash: string;
  exists: boolean;
  refused: string | null;
  repo: { url: string; environment: string; mount: string; stays: string[] } | null;
  no_repo: string | null;
  environment: string | null;
  vault: string | null;
  environments: string[];
  vaults: string[];
  vars: { name: string; on: string | null }[];
  missing: string[];
  model: string | null;
  left_out: LeftOut[];
  notes: string[];
}

/** What the run asks the daemon: each throws with the daemon's sentence. */
export interface FountainApi {
  // The daemon's routes for the Agents page; a block's calls for the block.
  preflight(args: Record<string, unknown>): Promise<Preflight>;
  copy(args: Record<string, unknown>): Promise<{ summary: string }>;
  run(args: Record<string, unknown>): Promise<{ block: number; summary: string }>;
}

/** `POST path`'s answer as JSON, or the daemon's `error` thrown. */
export async function post<T>(client: Client, path: string, body: unknown): Promise<T> {
  const r = await client.request("POST", path, body);
  const v = await r.json<T & { error?: string }>().catch(() => ({}) as T & { error?: string });
  if (!r.ok) throw new Error(v.error ?? `HTTP ${r.status}`);
  return v;
}

export function FountainRun({ recipe, cwd, api, close, ran }: { recipe: string; cwd: string | null; api: FountainApi; close: () => void; ran: (r: { block: number; summary: string }) => void }) {
  const [p, setP] = useState<Preflight | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [vault, setVault] = useState("");
  const [environment, setEnvironment] = useState("");
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState<string | null>("check");
  const [said, setSaid] = useState<string | null>(null);
  const args = () => ({ agent: recipe, ...(cwd ? { cwd } : {}), ...(vault ? { vault } : {}), ...(environment ? { environment } : {}) });
  const check = async () => {
    setBusy("check");
    try {
      setP(await api.preflight(args()));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
    setBusy(null);
  };
  useEffect(() => void check(), [vault, environment]);
  const copy = async (v: string) => {
    setBusy(`copy:${v}`);
    try {
      setSaid((await api.copy({ ...args(), var: v })).summary);
    } catch (e) {
      setSaid((e as Error).message);
    }
    setBusy(null);
    await check();
  };
  const run = async () => {
    setBusy("run");
    try {
      const r = await api.run({ ...args(), ...(prompt.trim() ? { prompt: prompt.trim() } : {}) });
      close();
      ran(r);
    } catch (e) {
      setError((e as Error).message);
      setBusy(null);
    }
  };
  const lost = [...(p?.left_out.map((l) => `${l.name}: ${l.why}`) ?? []), ...(p?.notes ?? [])];
  return (
    <div class="prompt-backdrop" role="dialog" aria-label={`Run ${recipe} on Fountain`} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <form
        class="prompt agent-dialog fountain-run"
        data-fountain-run={recipe}
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <h2>Run {recipe} on Fountain</h2>
        {!p && !error && <p class="hint">Checking Fountain…</p>}
        {error && (
          <p class="hint fountain-run-error" data-fountain-run-error>
            {error}
          </p>
        )}
        {p && (
          <>
            <p class="hint">
              As the Fountain agent <b>{p.agent}</b>
              {p.exists ? ", updated from the recipe if it changed" : ", made from the recipe now"}
              {p.model ? ` (${p.model})` : ""}. The conversation is labelled with the recipe's hash ({p.hash}).
            </p>
            {p.refused && (
              <p class="hint fountain-run-error" data-fountain-refused>
                {p.refused}
              </p>
            )}
            {p.repo ? (
              <p class="hint" data-fountain-repo>
                The sandbox clones {p.repo.url} (its default branch) into {p.repo.mount}, as the environment {p.repo.environment}. Uncommitted work on this machine doesn't travel
                {p.repo.stays.length ? `: ${p.repo.stays.join(", ")} stay here` : ""}.
              </p>
            ) : (
              p.no_repo && <p class="hint" data-fountain-repo>{p.no_repo}.</p>
            )}
            <label>
              Vault
              <select data-fountain-vault value={vault} onChange={(e) => setVault((e.currentTarget as HTMLSelectElement).value)}>
                <option value="">none</option>
                {p.vaults.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Environment
              <select data-fountain-environment value={environment} onChange={(e) => setEnvironment((e.currentTarget as HTMLSelectElement).value)}>
                <option value="">{p.repo ? `the project's (${p.repo.environment})` : "the agent's"}</option>
                {p.environments
                  .filter((e) => e !== p.repo?.environment)
                  .map((e) => (
                    <option key={e} value={e}>
                      {e}
                    </option>
                  ))}
              </select>
            </label>
            {p.missing.length > 0 && (
              <div class="fountain-run-missing" data-fountain-missing={p.missing.length}>
                <b>
                  {p.missing.length} variable{p.missing.length === 1 ? "" : "s"} missing on Fountain
                </b>
                <ul>
                  {p.missing.map((v) => (
                    <li key={v} data-missing-var={v}>
                      <code>{v}</code>{" "}
                      {vault ? (
                        <button type="button" data-copy-var={v} disabled={busy !== null} title="Read it on this machine (Infisical through agent-specs, your shell, gh) and write it to the vault once" onClick={() => void copy(v)}>
                          {busy === `copy:${v}` ? "Copying…" : `Copy from this machine to vault ${vault}`}
                        </button>
                      ) : (
                        <span class="dim">choose a vault to copy it to</span>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {said && (
              <p class="hint" data-fountain-said>
                {said}
              </p>
            )}
            {lost.length > 0 && (
              <details class="fountain-run-lost" data-fountain-lost={lost.length}>
                <summary>
                  What doesn't carry over as it is ({lost.length})
                </summary>
                <ul>
                  {lost.map((l) => (
                    <li key={l}>{l}</li>
                  ))}
                </ul>
              </details>
            )}
          </>
        )}
        <label>
          Prompt
          <textarea name="prompt" rows={3} value={prompt} placeholder="What should it do? (empty: just open it)" onInput={(e) => setPrompt(e.currentTarget.value)} />
        </label>
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" data-fountain-go disabled={busy !== null || !p || !!p.refused}>
            {busy === "run" ? "Starting…" : "Run on Fountain"}
          </button>
        </div>
      </form>
    </div>
  );
}

/** The Agents page's way to the daemon: its HTTP routes. */
export function pageApi(client: Client): FountainApi {
  return {
    preflight: (a) => post(client, "/api/a2a/fountain/preflight", a),
    copy: (a) => post(client, "/api/a2a/fountain/copy", a),
    run: (a) => post(client, "/api/a2a/run", { ...a, fountain: true }),
  };
}

/** An agents block's: its methods. */
export function blockApi(client: Client, id: number): FountainApi {
  return {
    preflight: (a) => post(client, `/api/blocks/${id}/call/fountain_preflight`, a),
    copy: (a) => post(client, `/api/blocks/${id}/call/fountain_copy`, a),
    run: (a) => post(client, `/api/blocks/${id}/call/run_fountain`, a),
  };
}
