// Settings (Arugula #1104, Lift 7): one list a scope, You at `/settings`,
// Project and Team, each with a picker for which one (../settings/scopes.tsx).
// Each section is a kit Row with what's set, opening in place
// (shared/ui/settings-list.tsx). The scope, which one and the section are
// the address (../paths.ts), so a link opens a section; an old link's
// `#section` lands on it too. A machine's settings are Machines' (a
// machine's Settings…), at their address here, with no tab of their own.

import { Tabs } from "../../shared/kit/tabs";
import { useSubscribe } from "../../shared/ui/hooks";
import { SettingsList } from "../../shared/ui/settings-list";
import { names } from "../links";
import { go, pathOf, type SettingsRoute } from "../router";
import { session } from "../session";
import { choicesOf, scopeRoute, SCOPE_KEYS, SCOPES, type SettingsScope } from "../settings/scopes";
import { useMachines } from "./machine-settings";

export function SettingsPage({ r }: { r: SettingsRoute }) {
  useSubscribe((fn) => session.subscribe(fn));
  const scope = SCOPES[r.scope] as SettingsScope;
  const at = (s: string | null): string => pathOf({ ...r, section: s ?? undefined, ...(r.scope === "project" && s !== "repos" ? { link: undefined } : {}) } as SettingsRoute, names);
  return (
    <div class="settings-host" data-settings-page={r.scope} {...hooksOf(r)}>
      <SettingsList
        label="Settings"
        section={r.section}
        hrefOf={at}
        go={go}
        header={
          <>
            <h1>{r.scope === "machine" ? "Machine settings" : "Settings"}</h1>
            {r.scope !== "machine" && (
              <div class="settings-scopes" data-settings-scopes>
                <Tabs
                  label="Settings for"
                  tabs={SCOPE_KEYS.filter((k) => scopeRoute(k)).map((k) => ({ id: k, title: SCOPES[k].title }))}
                  current={r.scope}
                  onPick={(k) => {
                    const next = k !== r.scope && scopeRoute(k as SettingsRoute["scope"]);
                    if (next) go(pathOf(next, names));
                  }}
                />
              </div>
            )}
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
