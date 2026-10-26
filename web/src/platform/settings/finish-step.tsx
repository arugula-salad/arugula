// A project's finish step (M261), in its settings: per hosted workspace, the
// command (`just check`) a thread's agent session runs after each turn and
// before it pushes. It's part of the workspace's spec (`PUT …/spec`), which
// otherwise says what the box is built from, but the box gets it with each
// pull of its sessions' work, so changing it needs no rebuild. One field a
// workspace; its timeout (15 minutes unless set) is the CLI's
// `arugula cloud edit --finish-timeout`.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { Section } from "./section";

/** A hosted workspace, as much of it as this needs. */
interface Workspace {
  id: string;
  name: string;
  spec: { finish?: string } & Record<string, unknown>;
}

async function call<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
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

export function FinishStep({ p }: { p: PlatformProject }) {
  const [list, setList] = useState<Workspace[] | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState("");
  const [said, setSaid] = useState<{
    ws: string;
    ok: boolean;
    text: string;
  } | null>(null);
  const base = `/api/projects/${encodeURIComponent(p.id)}/workspaces`;
  useEffect(() => {
    let live = true;
    call<{ workspaces: Workspace[] }>("GET", base).then(
      (v) => live && setList(v.workspaces),
      () => live && setList([]),
    );
    return () => void (live = false);
  }, [p.id]);
  if (!list?.length) return null;
  const changes = !p.team || p.team.role !== "viewer";
  const save = async (w: Workspace) => {
    const finish = (draft[w.id] ?? w.spec.finish ?? "").trim();
    setBusy(w.id);
    try {
      const got = await call<Workspace>(
        "PUT",
        `${base}/${encodeURIComponent(w.id)}/spec`,
        { ...w.spec, finish },
      );
      setList((l) => (l ?? []).map((x) => (x.id === w.id ? got : x)));
      setDraft(({ [w.id]: _, ...rest }) => rest);
      setSaid({
        ws: w.id,
        ok: true,
        text: finish
          ? "Saved: the next turn runs it."
          : "Saved: no finish step.",
      });
    } catch (e) {
      setSaid({ ws: w.id, ok: false, text: (e as Error).message });
    } finally {
      setBusy("");
    }
  };
  return (
    <Section
      id="finish"
      title="Finish step"
      lede="A command agent sessions run after each turn, before they push: its branch is pushed only if it passes."
      more={
        <p>
          It runs on the workspace's box as the session's own user, in its
          checkout, with nothing secret, for at most 15 minutes. When it fails,
          the agent gets one more turn to fix it, with the last of its output;
          when that fails too, nothing is pushed and the session's card says
          why. Changing it needs no rebuild.
        </p>
      }
    >
      {list.map((w) => {
        const value = draft[w.id] ?? w.spec.finish ?? "";
        const changed = value.trim() !== (w.spec.finish ?? "");
        return (
          <form
            key={w.id}
            class="platform-link-repo"
            aria-label={`The finish step on ${w.name}`}
            data-finish-step={w.name}
            onSubmit={(e) => {
              e.preventDefault();
              void save(w);
            }}
          >
            {list.length > 1 && <span class="sw-mono">{w.name}</span>}
            <input
              aria-label={`Finish step on ${w.name}`}
              placeholder="just check"
              value={value}
              disabled={!changes || busy === w.id}
              data-finish-input
              onInput={(e) => {
                const v = e.currentTarget.value;
                setDraft((d) => ({ ...d, [w.id]: v }));
              }}
            />
            {changes && (
              <button
                class="primary"
                disabled={busy === w.id || !changed}
                data-finish-save
              >
                Save
              </button>
            )}
            {said?.ws === w.id && (
              <span
                class={said.ok ? "dim" : "control-error"}
                role={said.ok ? undefined : "alert"}
              >
                {said.text}
              </span>
            )}
          </form>
        );
      })}
      {!changes && <p class="dim">A team's owners and editors set it.</p>}
    </Section>
  );
}
