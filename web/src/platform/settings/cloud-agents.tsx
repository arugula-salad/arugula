// Cloud agents, the account menu's dialog: the platform's agent library and
// cloud secrets (`CloudPanel`, ../cloud/cloud.tsx), opened on the catalog or
// on a run (`#cloud=run_…`). Drawn in control's overlay, which hands it the
// way to turn on push (M149).

import { Modal } from "../../shared/ui/modal";
import { CloudPanel } from "../cloud/cloud";

export function CloudAgents({ run, close, notify }: { run?: string; close: () => void; notify: () => Promise<string> }) {
  return (
    <Modal close={close}>
      <CloudPanel key={run ?? "catalog"} close={close} notify={notify} />
    </Modal>
  );
}
