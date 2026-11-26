// The fixture page for which machine opens first (Arugula #1425): the
// host directory as the app's page makes it, with the machine list control
// would send, and nothing else. first-machine.spec.ts builds this with Vite.

import { directory, type Host } from "../src/shell/hosts";

(window as unknown as { __directory: unknown }).__directory = directory;
(window as unknown as { __listed(hosts: Host[]): string }).__listed = (hosts) => {
  directory.setControl(hosts, false);
  return directory.current;
};
