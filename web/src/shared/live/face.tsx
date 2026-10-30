// A person's live face over their picture (Arugula #344): chat's avatars
// (../chat/avatar) and the shell's. While the live presence store (./live)
// has them live (their camera on, not away) in a room this page is in, it
// draws them there with arugula.stream's `room.avatar`, always as their
// character at these sizes (decided 2026-10-29: an outline doesn't read
// at 24-32 px). Only while it's on screen, so a long history of their
// messages runs as many faces as show, not one per message. Not live:
// nothing, and the picture under it shows.

import { useEffect, useRef, useState } from "preact/hooks";
import { useSubscribe } from "../ui/hooks";
import { live } from "./live";
import "./face.css";

export function LiveFace({ account }: { account: string }) {
  useSubscribe((fn) => live.subscribe(fn));
  const face = live.live(account);
  const room = face?.room ?? null;
  const ref = useRef<HTMLSpanElement>(null);
  const [onScreen, setOnScreen] = useState(false);
  // Watched only while there's a face to draw.
  useEffect(() => {
    const el = ref.current;
    if (!room || !el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver((seen) => setOnScreen(seen.some((s) => s.isIntersecting)));
    io.observe(el);
    return () => io.disconnect();
  }, [room]);
  useEffect(() => {
    const el = ref.current;
    if (!room || !onScreen || !el) return;
    return room.avatar(el, account, { size: "head", draw: "character" });
  }, [room, onScreen, account]);
  return <span ref={ref} class={`live-face${room && onScreen ? " on" : ""}`} data-live-face={room ? account : undefined} aria-hidden="true" />;
}

/** An account id from a shell presence's principal (`account:<id>`), or
 * null for one that isn't an account (`owner`, `tailnet:…`). */
export function accountOf(who: string): string | null {
  return who.startsWith("account:") ? who.slice("account:".length) : null;
}
