// @arugula/presence-react (https://arugula.stream/docs#react).
import type { CSSProperties, ReactNode, JSX } from "react";

export interface Look {
  character: number;
  colour: number;
}

export interface Member {
  sub: string;
  name: string;
  look: Look | null;
  mode: "outline" | "avatar" | "none";
  away: boolean;
  watching: boolean;
}

export interface PresenceRoomProps {
  /** The room's id in your app. */
  room: string;
  /** A room token your server signed, or a function for a fresh one (called on every (re)connect). */
  token: string | (() => Promise<string>);
  /** Default: wss://arugula.stream/socket. */
  relay?: string;
  mode?: "avatar" | "outline";
  look?: Look;
  children?: ReactNode;
}

export function PresenceRoom(props: PresenceRoomProps): JSX.Element;

export function People(props: { size?: "bust" | "head"; self?: "first" | "hide"; className?: string }): JSX.Element;

export function Avatar(props: { user: string; size?: "head" | "bust"; className?: string; style?: CSSProperties }): JSX.Element;

export function CameraControls(props: { controls?: "full" | "button"; className?: string }): JSX.Element;

export function usePresence(): {
  members: Member[];
  live: "connecting" | "on" | "refused" | "off";
  error: Error | null;
  me: string;
  waves: { from: string; at: number }[];
  wave(sub: string): void;
  /** An emoji for the room; it floats over your tile for everyone. */
  react(emoji: string): void;
  reactions: { from: string; emoji: string; at: number }[];
  answer(sub: string, yes: boolean): void;
  setLook(look: Look): void;
  camera: { on: boolean; start(o?: { mode?: "avatar" | "outline" }): Promise<void> | void; stop(): void };
};
