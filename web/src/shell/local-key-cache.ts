// Where this daemon's page leaves its page key for the service worker,
// which has no localStorage (local-key.ts writes it, sw.ts reads it). A
// module of its own: the worker is a classic script, and local-key.ts holds
// the app back while it signs in (a top-level await).

export const KEY_CACHE = "arugula-key";
