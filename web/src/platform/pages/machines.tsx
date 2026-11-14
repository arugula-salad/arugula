// The machines in a project (Arugula #989): the app draws them where it
// reaches machines (`PlatformApi.Machines`). Until then, or on a page that
// can't, the same place opens on control's page, as the links did before.

import { useEffect } from "preact/hooks";
import { controlHash, parseMachines, type MachineAt } from "../../shared/machine-paths";
import { usePlatformApi } from "../../shared/platform-api";
import { controlOrigin } from "../session";

export function MachinesPage({ project, at }: { project: string; at: MachineAt }) {
  const { Machines } = usePlatformApi();
  return Machines ? <Machines project={project} at={at} /> : <OnControl at={at} />;
}

/** Control's page at `at`, in this window. */
export function OnControl({ at }: { at: MachineAt }) {
  // A machine's own place (`#overview`, `#settings=machine`) rides in the fragment.
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
