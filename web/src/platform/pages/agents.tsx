// `/agents`: the cloud agent library and cloud secrets (M150), the panel
// the shell's account menu opens as a dialog, on a page of its own here.
// Push stays on the hub's app ("Push stays on the hub's app"), so this page
// offers no notifications of its own.

import { CloudPanel } from "../cloud/cloud";
import { go } from "../router";

export function AgentsPage() {
  return (
    <section class="platform-section platform-agents" data-platform-agents>
      <CloudPanel close={() => go("/")} />
    </section>
  );
}
