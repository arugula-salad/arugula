import type { Client } from "../client";
import { openMenu, type MenuItem } from "../../shared/ui/menu";

/** A menu whose items depend on what the daemon is set up for: read that
 * again first (briefly), so a studio linked a moment ago shows (#180). */
export function openFresh(client: Client, e: { clientX: number; clientY: number; preventDefault(): void }, build: () => MenuItem[]) {
  e.preventDefault();
  const at = { clientX: e.clientX, clientY: e.clientY, preventDefault() {} };
  const wait = new Promise((r) => setTimeout(r, 300));
  void Promise.race([client.loadFeatures(), wait]).then(() => openMenu(at, build()));
}
