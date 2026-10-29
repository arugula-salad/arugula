// Who's around as outlines or avatars (S43, S44), for every People block
// and chat avatar on the page: @arugula/presence's client over Arugula
// control's relay (crates/platform/src/chat/outlines.rs), which shows each
// person the people they share a team with. Your mode and look are kept
// in this browser, under the keys the spike used.

// By path, not the package's index: the index has the Phoenix transport,
// which Arugula doesn't use.
import { PresenceClient } from "@arugula/presence/client";
import { arugulaTransport } from "@arugula/presence/arugula";
import { platformFetch } from "../maintenance";

export type { Seen, Mine } from "@arugula/presence/client";

export const outlines = new PresenceClient({ transport: arugulaTransport({ fetch: platformFetch }), storage: "arugula.s44." });
