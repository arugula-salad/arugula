// A calm note while control's chat and Cloud are in a maintenance window
// (M141): calls to them wait in the background (../maintenance.ts), and
// terminals keep working, so it's a status, not an error. It goes once a
// call gets through.

import { maintenance, MAINTENANCE_SAID } from "../maintenance";
import { useSubscribe } from "./hooks";

export function MaintenanceNote() {
  useSubscribe((fn) => maintenance.subscribe(fn));
  if (!maintenance.down) return null;
  return (
    <div class="ws-stale maintenance-note" role="status" data-maintenance-note>
      {MAINTENANCE_SAID} Terminals work as usual.
    </div>
  );
}
