// The account's email for security notices (#1609), in Sign-in and
// account: the shared part (../../shared/ui/account-email.tsx), sent
// through control's page. The desktop app's window shows it, and changes
// happen in a browser (control refuses the window).

import { api } from "../control";
import { desktopApp } from "../../shared/desktop";
import type { AccountCall } from "../../shared/account";
import { EmailSection as Email } from "../../shared/ui/account-email";

/** Where the verify link comes back to: `#email=verified` or `#email=expired`. */
export const EMAIL_HASH = /^#email=(verified|expired)$/;

/** `/api/me/*` on control's own page. */
export const shellCall: AccountCall = async <T,>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown): Promise<T> => {
  if (method === "DELETE") {
    const r = await fetch(path, { method });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return (await r.json().catch(() => ({}))) as T;
  }
  return api<T>(path, method === "POST" ? (body ?? {}) : undefined);
};

export function EmailSection({ back }: { back?: string }) {
  return <Email call={shellCall} back={back} readOnly={!!desktopApp()} />;
}
