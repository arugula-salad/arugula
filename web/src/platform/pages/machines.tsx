// The machines in a project (Arugula #989): the app draws them where it
// reaches machines (`PlatformApi.Machines`). Until then, or on a page that
// can't, the same place opens on control's page, as the links did before.
// But Settings over the machines (control's `#settings=…`, moved here) is
// the app's Settings (Arugula #1110).

import { useEffect } from "preact/hooks";
import { controlHash, parseMachines, settingsPath, type MachineAt } from "../../shared/machine-paths";
import { usePlatformApi } from "../../shared/platform-api";
import { go } from "../router";
import { controlOrigin } from "../session";

export function MachinesPage({ project, at }: { project: string; at: MachineAt }) {
  const { Machines } = usePlatformApi();
  const settings = at.at === "machines" ? inSettings(location.hash) : null;
  useEffect(() => {
    if (settings) go(settings, true);
  }, [settings]);
  if (settings) return null;
  return Machines ? <Machines project={project} at={at} /> : <OnControl at={at} />;
}

/** Where `#settings=<section>` over the machines is in Settings: a
 * machine's own at This machine's first (the list names no machine), and
 * Phones and browsers at You's Notifications, whose row leads to
 * control's page. */
function inSettings(hash: string): string | null {
  const m = /^#settings(?:=([\w-]+))?$/.exec(hash);
  return m ? (settingsPath(m[1]) ?? "/settings/you/notifications") : null;
}

/** Control's page at `at`, in this window. */
export function OnControl({ at }: { at: MachineAt }) {
  // A machine's own place (`#agents`, `#settings=machine`) rides in the fragment.
  const here = at.at === "machines" ? (parseMachines([], new URLSearchParams(location.search), location.hash) ?? at) : at;
  const hash = controlHash(here);
  const href = `${controlOrigin()}/${hash ? `#${hash}` : ""}`;
  useEffect(() => location.replace(href), [href]);
  return (
    <p class="platform-section dim" data-machines-elsewhere>
      Opening it on <a href={href}>your machines</a>…
    </p>
  );
}
