// A person's rigged avatar on a canvas (S44): theirs from the stream (the
// newest rig, or idling in the look they last sent once it goes stale),
// yours from your own camera in avatar mode, idling in your look when the
// camera's off. Dozes while its person is away (chat's presence).

import { useEffect, useRef } from "preact/hooks";
import { presence } from "../chat/presence";
import { outlines } from "../outline/store";
import { characterOf } from "@arugula/presence-core/avatar/characters/index";
import { animate, type Source } from "@arugula/presence-core/avatar/stage";

/** What `account`'s avatar draws from now, or null when they have none
 * here (never sent a rig, and aren't you in avatar mode). */
export function avatarSource(account: string, me: boolean): Source | null {
  const away = presence.of(account) === "away";
  if (me && outlines.mode === "avatar") return { rig: outlines.mine?.rig ?? null, look: outlines.look, away };
  const seen = outlines.of(account);
  const look = seen?.look ?? outlines.lookOf(account);
  if (!look) return null;
  return { rig: seen?.rig ?? null, look, away };
}

/** Whether `account` has an avatar to show here. */
export function hasAvatar(account: string, me: boolean): boolean {
  return avatarSource(account, me) !== null;
}

export function AvatarCanvas({ account, me, size, class: cls }: { account: string; me: boolean; size: "bust" | "head"; class?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    return animate(c, { size, mirror: me, character: characterOf, source: () => avatarSource(account, me) });
  }, [account, me, size]);
  return <canvas ref={ref} class={cls} data-avatar={account} aria-hidden="true" />;
}
