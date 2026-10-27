// Which model a project's agent sessions use (Arugula #215), in its
// settings: "agents use model …" (`GET`/`PUT
// /api/projects/:project/sessions/settings`). The strongest unless it says
// (Opus, for Claude Code); an ask's own word ("use sonnet") wins for its
// session. Codex picks its own model whatever this says. Its owners and
// editors change it; it counts from the next session on.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { SessionSettings } from "../../shared/proto";
import type { PlatformProject } from "../session";
import { Section } from "./section";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

const name = (m: string) => m.charAt(0).toUpperCase() + m.slice(1);

export function AgentModel({ p }: { p: PlatformProject }) {
  const [now, setNow] = useState<SessionSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null);
  const path = `/api/projects/${encodeURIComponent(p.id)}/sessions/settings`;
  useEffect(() => {
    let live = true;
    call<SessionSettings>("GET", path).then(
      (v) => live && setNow(v),
      () => live && setNow(null),
    );
    return () => void (live = false);
  }, [path]);
  if (!now) return null;
  const changes = !p.team || p.team.role !== "viewer";
  const choose = async (model: string | null) => {
    setBusy(true);
    try {
      setNow(await call<SessionSettings>("PUT", path, { model, models: [] }));
      setSaid({ ok: true, text: "Saved: the next session uses it." });
    } catch (e) {
      setSaid({ ok: false, text: (e as Error).message });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section
      id="agent-model"
      title="Agent model"
      lede="The model agent sessions use: the strongest unless you choose another."
      more={
        <p>
          The strongest is Opus, for Claude Code. An ask can name its own, as in “use sonnet”, and that wins for
          its session. Codex picks its own model whatever this says. Each session's card says the model its agent runs on.
        </p>
      }
    >
      <div class="platform-link-repo" data-agent-model>
        <select
          class="field"
          aria-label="The model agent sessions use"
          value={now.model ?? ""}
          disabled={!changes || busy}
          data-agent-model-select
          onChange={(e) => {
            const v = e.currentTarget.value;
            void choose(v === "" ? null : v);
          }}
        >
          <option value="">The strongest ({name(now.models[0] ?? "opus")})</option>
          {now.models.map((m) => (
            <option key={m} value={m}>
              {name(m)}
            </option>
          ))}
        </select>
        {said && (
          <span class={said.ok ? "dim" : "control-error"} role={said.ok ? undefined : "alert"}>
            {said.text}
          </span>
        )}
      </div>
      {!changes && <p class="dim">A team's owners and editors choose it.</p>}
    </Section>
  );
}
