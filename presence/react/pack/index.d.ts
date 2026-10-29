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
  /** The call they're in, if any. */
  call: string | null;
}

/** Someone in your call, you included. */
export interface CallPerson {
  sub: string;
  name: string;
  muted: boolean;
  speaking: boolean;
  audio: boolean;
}

/** Your call, or the one you're ringing someone into. */
export interface Call {
  id: string;
  state: "ringing" | "joining" | "on" | "reconnecting";
  ringing: string[];
  people: CallPerson[];
  muted: boolean;
  blocked: boolean;
  error: string;
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

export function People(props: { size?: "bust" | "head"; self?: "first" | "hide"; calls?: boolean; className?: string }): JSX.Element;

export function Avatar(props: { user: string; size?: "head" | "bust"; className?: string; style?: CSSProperties }): JSX.Element;

export function CameraControls(props: { controls?: "full" | "button"; className?: string }): JSX.Element;

/** The call bar: a ring to answer, who you're calling, mute and leave. */
export function CallControls(props: { className?: string }): JSX.Element;

export interface Message {
  id: number;
  sub: string;
  name: string;
  text: string;
  at: number;
}

/** The room's text: messages, earlier ones a click away, and an input. */
export function Chat(props: { placeholder?: string; className?: string; style?: CSSProperties }): JSX.Element;

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
  /** The room's text messages, oldest first. */
  messages: Message[];
  /** Send a message; resolves with it as kept, rejects with why. */
  say(text: string): Promise<Message>;
  /** The page of messages before the first one here. */
  loadEarlier(): Promise<void> | void;
  answer(sub: string, yes: boolean): void;
  /** Whether this relay has calls (hide call buttons where not). */
  calls: boolean;
  /** Who's ringing you, until answered or over. */
  rings: { from: string; id: string; at: number }[];
  /** Your call, or null. */
  inCall: Call | null;
  /** Ask someone to talk: a voice call once they say yes. Resolves with the call's id. */
  call(sub: string): Promise<string>;
  answerCall(id: string, yes: boolean): Promise<void> | void;
  leaveCall(): void;
  setMuted(muted: boolean): Promise<void> | void;
  setLook(look: Look): void;
  camera: { on: boolean; start(o?: { mode?: "avatar" | "outline" }): Promise<void> | void; stop(): void };
};
