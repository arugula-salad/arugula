// Settings (Arugula #1104): one page, four scopes (./../settings/scopes.tsx):
// You at `/settings`, This machine, Project, and Team and billing, each
// with a picker for which one. A scope is a list of its sections, each
// with what's set, and the chosen one's controls beside it
// (shared/ui/settings-list.tsx, layout B of the Settings Doc). The scope,
// which one and the section are the address (../paths.ts), so a link opens
// a section; an old link's `#section` lands on it too.

import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { SettingsList } from "../../shared/ui/settings-list";
import { names } from "../links";
import { follow, go, pathOf, type SettingsRoute } from "../router";
import { session } from "../session";
import { choicesOf, scopeRoute, SCOPE_KEYS, SCOPES, type SettingsScope } from "../settings/scopes";
import { useMachines } from "./machine-settings";

export function SettingsPage({ r }: { r: SettingsRoute }) {
  useSubscribe((fn) => session.subscribe(fn));
  const scope = SCOPES[r.scope] as SettingsScope;
  const at = (s: string | null): string => pathOf({ ...r, section: s ?? undefined, ...(r.scope === "project" && s !== "repos" ? { link: undefined } : {}) } as SettingsRoute, names);
  const whose = r.scope === "project" ? session.project(r.project)?.name : r.scope === "team" ? choicesOf(r).find((c) => c.id === r.owner)?.name : undefined;
  return (
    <div class="settings-host" data-settings-page={r.scope} {...hooksOf(r)}>
      <SettingsList
        label="Settings"
        section={r.section}
        hrefOf={at}
        open={follow}
        back={whose ?? scope.title}
        header={
          <>
            <PageHeader title="Settings" sub={scope.whose}>
              <Scopes r={r} />
            </PageHeader>
            {scope.pick && <ScopePick r={r} label={scope.pick} />}
          </>
        }
      >
        {(scope.sections as (r: SettingsRoute) => preact.ComponentChildren)(r)}
      </SettingsList>
    </div>
  );
}

/** The `data-*` hooks the scope's page had before it was a scope. */
function hooksOf(r: SettingsRoute): Record<string, string> {
  if (r.scope === "you") return { "data-platform-settings": "" };
  if (r.scope === "project") return { "data-project-settings": r.project };
  if (r.scope === "team") return { "data-owner-settings": r.owner };
  return { "data-machine-settings-page": r.machine ?? "" };
}

/** The scopes, as tabs: each opens where `scopeRoute` says. */
function Scopes({ r }: { r: SettingsRoute }) {
  const keys = SCOPE_KEYS;
  const to = (i: number) => {
    const next = scopeRoute(keys[i]);
    if (next) go(pathOf(next, names));
  };
  return (
    <div class="seg small settings-scopes" role="tablist" aria-label="Settings for">
      {keys.map((k, i) => (
        <button
          key={k}
          type="button"
          role="tab"
          id={`settings-scope-${k}`}
          aria-selected={r.scope === k}
          tabIndex={r.scope === k ? 0 : -1}
          disabled={!scopeRoute(k)}
          data-settings-scope={k}
          onClick={() => r.scope !== k && to(i)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
            const n = (i + (e.key === "ArrowRight" ? 1 : keys.length - 1)) % keys.length;
            document.getElementById(`settings-scope-${keys[n]}`)?.focus();
          }}
        >
          {SCOPES[k].title}
        </button>
      ))}
    </div>
  );
}

/** Which machine, project or team: the scope's picker. */
function ScopePick({ r, label }: { r: SettingsRoute; label: string }) {
  const { machines } = useMachines();
  const choices = r.scope === "machine" ? (machines ?? []) : choicesOf(r);
  const value = r.scope === "machine" ? (r.machine ?? "") : r.scope === "project" ? r.project : r.scope === "team" ? r.owner : "";
  const pick = (id: string) => {
    // The same section in the next one, where it has it; the list falls back to its first.
    const next: SettingsRoute =
      r.scope === "machine"
        ? { page: "settings", scope: "machine", machine: id, section: r.section }
        : r.scope === "project"
          ? { page: "settings", scope: "project", project: id, section: r.section }
          : { page: "settings", scope: "team", owner: id, section: r.section };
    go(pathOf(next, names));
  };
  return (
    <label class="settings-pick">
      <span>{label}</span>
      <select class="field" value={value} data-settings-pick={r.scope} disabled={choices.length === 0} onChange={(e) => pick(e.currentTarget.value)}>
        {!choices.some((c) => c.id === value) && <option value={value}>{r.scope === "machine" && !machines ? "Loading…" : "Choose one"}</option>}
        {choices.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
    </label>
  );
}
