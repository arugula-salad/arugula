// @channel, @here and keyword alerts (#114), what the page knows of them:
// the words the composer offers after the people, the confirm in a big
// room, and the account's keywords and a team's owners-only switch, which
// control keeps.

import type { ChatKeywords, ChatTeamSettings } from "../proto";
import { call } from "../chat-store";

/** Rooms with more people than this ask before `@channel` or `@here`. */
export const BROADCAST_CONFIRM_OVER = 6;

export const BROADCASTS = [
  { token: "channel", note: "Notify everyone in this room" },
  { token: "here", note: "Notify those active now" },
] as const;

/** Whether a text has `@channel` or `@here` as control reads them: after
 * the start or anything but a letter or digit. */
export function usesBroadcast(text: string): boolean {
  return /(?:^|[^\p{L}\p{N}])@(?:channel|here)(?![\p{L}\p{N}_.-])/iu.test(text);
}

/** The offers for what's typed after `@`: `channel` and `here`, those that
 * start with it. */
export function broadcastOptions(typed: string): (typeof BROADCASTS)[number][] {
  const t = typed.toLowerCase();
  return BROADCASTS.filter((b) => b.token.startsWith(t));
}

export function keywords(): Promise<ChatKeywords> {
  return call<ChatKeywords>("GET", "/api/chat/keywords");
}

export function setKeywords(words: string[]): Promise<ChatKeywords> {
  return call<ChatKeywords>("PUT", "/api/chat/keywords", { words });
}

export function teamSettings(team: string): Promise<ChatTeamSettings> {
  return call<ChatTeamSettings>("GET", `/api/chat/teams/${encodeURIComponent(team)}/settings`);
}

export function setTeamSettings(team: string, s: ChatTeamSettings): Promise<ChatTeamSettings> {
  return call<ChatTeamSettings>("PUT", `/api/chat/teams/${encodeURIComponent(team)}/settings`, s);
}
