// Sign-in and account's words and shapes, as control's dialog
// (shell/trust/account.tsx) and the app's Settings › You › Sign-in and
// security (platform/pages/you-security.tsx) both read them: what a
// sign-in or a passkey is called, and what control's `/api/me/*` answers.

/** One place the account is signed in (`GET /api/me/sessions`). */
export interface SessionRow {
  id: string;
  created: number;
  expires: number;
  agent: string;
  /** The account's device it is, once its page said (null before). */
  device?: string | null;
  current: boolean;
}

/** `GET /api/me/passkeys`: the account's passkeys, its GitHub login, and
 * how many ways in it has (never fewer than one). */
export interface Passkeys {
  passkeys: { id: string; created: number; agent: string; provider: string | null; used: number | null; prf: boolean | null; device: string | null }[];
  github: string | null;
  ways: number;
}

/** `GET /api/me/delete`: what deleting the account takes with it, and
 * what stands in the way. */
export interface DeletePreview {
  confirm: string;
  disband: { team: string; name: string; others: number }[];
  blockers: string[];
  pays: string[];
  machines: number;
  stays_in: string[];
}

/** A call to control's `/api/me/*`: its answer, or its `{error}` thrown.
 * Each page sends it its own way (the hub's page, or the app's host). */
export type AccountCall = <T>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown) => Promise<T>;

/** "Safari on iPhone", from a User-Agent: what a browser calls itself as a
 * device, and what Sign-in calls a sign-in no device has claimed, so the
 * two lists use the same words. */
export function agentName(ua: string): string {
  if (/arugula/i.test(ua) && !/Mozilla/.test(ua)) return "Arugula app";
  const os = /iPhone/.test(ua)
    ? "iPhone"
    : /iPad/.test(ua)
      ? "iPad"
      : /Android/.test(ua)
        ? "Android"
        : /Mac/.test(ua)
          ? "Mac"
          : /Windows/.test(ua)
            ? "Windows"
            : /Linux|X11/.test(ua)
              ? "Linux"
              : "";
  const app = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Epiphany\//.test(ua)
          ? "GNOME Web"
          : /Safari\//.test(ua)
            ? // Safari's token off Apple's systems is WebKitGTK: the Linux
              // app's window (when it hasn't said its own name) or GNOME
              // Web, never Safari (#817).
              os === "Linux"
              ? "WebKit"
              : "Safari"
            : /node|undici|curl|reqwest/i.test(ua)
              ? "A program"
              : "";
  if (!app && !os) return "A browser";
  return app && os ? `${app} on ${os}` : app || `A browser on ${os}`;
}

/** What a sign-in or passkey came from, in the words Devices uses for a
 * browser ("Safari on iPhone"). Sign-ins from before control kept the
 * browser have none. */
export function describeAgent(ua: string): string {
  if (!ua) return "An earlier sign-in";
  const name = agentName(ua);
  // UI audit #195: a sign-in from no browser, named by what it is. The CLI
  // keeps none (it signs each call with its device key), and the desktop
  // app's window names its device once its page loads (sessionName), so
  // what's left signed in without a browser is a headless device (the
  // tests' and dev-control's) or a script: named by its runtime's word.
  const runtime = name === "A program" ? /\b(node|undici|curl|reqwest)\b/i.exec(ua)?.[1] : null;
  return runtime ? `A headless device (${runtime.toLowerCase()})` : name;
}

/** What keeps a passkey (iCloud Keychain, Windows Hello...) when control
 * knows its maker, else the browser that added it (#208). Ones added
 * before either was kept are numbered. */
export function passkeyName(k: Passkeys["passkeys"][number], i: number): string {
  if (k.provider) return k.provider;
  if (k.agent) return `Passkey from ${describeAgent(k.agent)}`;
  return `Passkey ${i + 1}`;
}

/** What linking GitHub came back with, for the page to say. */
export const LINKED: Record<string, string> = {
  linked: "GitHub is linked: you can sign in with it too.",
  taken: "That GitHub account already signs in to another Arugula account. Nothing changed.",
  "has-other": "This account has another GitHub account linked: unlink it first.",
  elsewhere: "That link was started in another browser or sign-in: press Link GitHub again here.",
  expired: "That link took too long or was used already: press Link GitHub again.",
  declined: "GitHub didn't link: it was declined on GitHub.",
  failed: "Linking GitHub failed: try again.",
};
