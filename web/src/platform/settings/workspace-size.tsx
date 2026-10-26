// A project's cloud workspaces' sizes (M266), in its settings: per hosted
// workspace, small (2 vCPU, 2 GiB) or large (8 vCPU, 8 GiB). E2B fixes a
// box's size when its template is built, so each size is a template of its
// own and changing it takes a rebuild: it's part of the workspace's spec
// (`PUT …/spec`), and the workspace shows "Wants a rebuild" until then.
// The CLI's is `arugula cloud edit --size`. Owners and editors change it.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { CopyText } from "../../shared/ui/copy";
import type { PlatformProject } from "../session";
import { Section } from "./section";

export type Size = "small" | "large";

/** Each size, as the platform's `Size` is built (compute/template.rs). */
export const SIZES: { size: Size; label: string; box: string; per_hour: string }[] = [
  { size: "small", label: "Small: 2 vCPU, 2 GiB", box: "2 vCPU, 2 GiB", per_hour: "$0.13" },
  { size: "large", label: "Large: 8 vCPU, 8 GiB", box: "8 vCPU, 8 GiB", per_hour: "$0.53" },
];

/** A spec's size: small unless it says (a small spec leaves it out). */
export const sizeOf = (spec: { size?: string }): Size => (spec.size === "large" ? "large" : "small");

/** A hosted workspace, as much of it as this needs. */
interface Workspace {
  id: string;
  name: string;
  spec: { size?: Size } & Record<string, unknown>;
  rebuild_needed: boolean;
}

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

export function WorkspaceSize({ p }: { p: PlatformProject }) {
  const [list, setList] = useState<Workspace[] | null>(null);
  const [draft, setDraft] = useState<Record<string, Size>>({});
  const [busy, setBusy] = useState("");
  const [said, setSaid] = useState<{ ws: string; ok: boolean; text: string } | null>(null);
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
    const size = draft[w.id] ?? sizeOf(w.spec);
    setBusy(w.id);
    try {
      const got = await call<Workspace>("PUT", `${base}/${encodeURIComponent(w.id)}/spec`, { ...w.spec, size });
      setList((l) => (l ?? []).map((x) => (x.id === w.id ? got : x)));
      setDraft(({ [w.id]: _, ...rest }) => rest);
      setSaid({
        ws: w.id,
        ok: true,
        text: got.rebuild_needed ? `Saved: its box is ${size} once it's rebuilt.` : `Saved: its box is ${size}.`,
      });
    } catch (e) {
      setSaid({ ws: w.id, ok: false, text: (e as Error).message });
    } finally {
      setBusy("");
    }
  };
  return (
    <Section
      id="size"
      title="Workspace size"
      lede="How big each cloud workspace's box is. Changing it takes a rebuild."
      more={
        <>
          <p>
            Small is 2 vCPU and 2 GiB, large 8 vCPU and 8 GiB, enough for a Rust build. While its box runs, small costs about{" "}
            {SIZES[0].per_hour} an hour and large about {SIZES[1].per_hour}, at E2B's prices; a paused box costs nothing.
          </p>
          <p>
            A rebuild makes its box again from its spec: its disk starts over. Agent sessions on it carry on in the new box, from their
            threads.
          </p>
        </>
      }
    >
      {list.map((w) => {
        const now = sizeOf(w.spec);
        const value = draft[w.id] ?? now;
        return (
          <form
            key={w.id}
            class="platform-link-repo"
            aria-label={`The size of ${w.name}`}
            data-workspace-size={w.name}
            onSubmit={(e) => {
              e.preventDefault();
              void save(w);
            }}
          >
            {list.length > 1 && <span class="sw-mono">{w.name}</span>}
            <select
              class="field"
              aria-label={`Size of ${w.name}`}
              value={value}
              disabled={!changes || busy === w.id}
              data-size-select
              onChange={(e) => {
                const v = e.currentTarget.value as Size;
                setDraft((d) => ({ ...d, [w.id]: v }));
              }}
            >
              {SIZES.map((s) => (
                <option key={s.size} value={s.size}>
                  {s.label}
                </option>
              ))}
            </select>
            {changes && (
              <button class="primary" disabled={busy === w.id || value === now} data-size-save>
                Save
              </button>
            )}
            {value !== now && <span class="dim">Takes a rebuild.</span>}
            {said?.ws === w.id && (
              <span class={said.ok ? "dim" : "control-error"} role={said.ok ? undefined : "alert"} data-size-said>
                {said.text}
              </span>
            )}
            {w.rebuild_needed && (
              <span data-size-rebuild>
                <span class="sw-chip">Wants a rebuild</span>
                <CopyText text={`arugula cloud rebuild ${w.name} --project ${p.id}`} />
              </span>
            )}
          </form>
        );
      })}
      {!changes && <p class="dim">A team's owners and editors set it.</p>}
    </Section>
  );
}
