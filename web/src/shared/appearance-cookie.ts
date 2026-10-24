// The appearance across arugula.io's hosts: control.arugula.io (the shell)
// and app.arugula.io (the platform page, M150) are two origins, and
// localStorage is per origin, so a palette picked on one wasn't on the
// other. On arugula.io the choice is also a cookie on the parent domain,
// holding only the appearance's JSON. Anywhere else (127.0.0.1 in dev, a
// daemon's own page, the desktop app's local origin) there's no cookie and
// localStorage alone keeps it, as before. No imports, so
// scripts/appearance-cookie.test.mjs runs it under node.

/** The parent domain the cookie goes on, or null where there's none. */
export function cookieDomain(host: string): string | null {
  const h = host.toLowerCase().replace(/\.$/, "");
  return h === "arugula.io" || h.endsWith(".arugula.io") ? "arugula.io" : null;
}

/** `name`'s value in a `document.cookie` string, or null when it's missing
 * or doesn't decode. */
export function readCookie(cookies: string, name: string): string | null {
  for (const part of cookies.split(";")) {
    const at = part.indexOf("=");
    if (at < 0 || part.slice(0, at).trim() !== name) continue;
    try {
      return decodeURIComponent(part.slice(at + 1).trim());
    } catch {
      return null;
    }
  }
  return null;
}

/** A year: Chrome caps a cookie at 400 days, and every load renews it. */
const MAX_AGE = 365 * 24 * 60 * 60;

/** The `document.cookie` line that sets `name` to `value` on `domain`. */
export function cookieLine(name: string, value: string, domain: string): string {
  return `${name}=${encodeURIComponent(value)}; Domain=${domain}; Path=/; Max-Age=${MAX_AGE}; Secure; SameSite=Lax`;
}

/** The first of the cookie's and localStorage's copies that `parse`
 * accepts, cookie first: it's the one every arugula.io host shares. */
export function pickStored<T>(cookie: string | null, stored: string | null, parse: (raw: string) => T | null): T | null {
  for (const raw of [cookie, stored]) {
    if (raw == null) continue;
    const v = parse(raw);
    if (v) return v;
  }
  return null;
}
