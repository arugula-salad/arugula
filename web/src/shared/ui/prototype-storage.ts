// The browser's storage for a Doc prototype's kept settings (#508), per
// viewer and on this page's origin. Only the app and the platform page import
// this: the share page's chunks must hold no browser storage (#1064), so a
// shared link keeps its settings in memory (`prototype-bridge.ts`).

import { setStoreBackend } from "./prototype-bridge";

setStoreBackend({
  get: (k) => localStorage.getItem(k),
  set: (k, v) => localStorage.setItem(k, v),
});
