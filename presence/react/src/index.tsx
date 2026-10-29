// @arugula/presence-react: arugula.stream for React (../../sdk/API.md).
// <PresenceRoom> joins a room with connect() and shares it through
// context; <People>, <CameraControls> and <Avatar> draw into elements of
// their own with the same code the custom elements use; <CallControls>
// is the call bar; usePresence() gives the room's members, calls and
// actions to your components.

import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { connect, styled, type ConnectOptions, type Room } from "@arugula/presence-elements/room";
import { mountCall, mountCamera, type CameraOptions } from "@arugula/presence-elements/controls";
import type { AvatarOptions, PeopleOptions } from "@arugula/presence-elements/view";
import type { Call, Member, Message } from "@arugula/presence";
import type { ChatOptions } from "@arugula/presence-elements/chat";
import type { Look } from "@arugula/presence-core/avatar/rig";

interface Ctx {
  room: Room | null;
  members: Member[];
  live: "connecting" | "on" | "refused" | "off";
  error: Error | null;
}

const Presence = createContext<Ctx>({ room: null, members: [], live: "off", error: null });

export interface PresenceRoomProps {
  room: string;
  /** A room token your server signed, or a function for a fresh one. */
  token?: ConnectOptions["token"];
  /** Or, with no server: the app's publishable key (connect()'s `key`;
   * React keeps `key` for itself), with `auth` or `guest`. */
  publishableKey?: string;
  /** The person's token from your auth provider, or a function for a
   * fresh one (called on every (re)connect). */
  auth?: ConnectOptions["auth"];
  /** A guest's name (with publishableKey and no auth). */
  guest?: string;
  /** A member's name, if their token has none. */
  name?: string;
  relay?: string;
  mode?: "avatar" | "outline";
  look?: Look;
  children?: ReactNode;
}

/** Join `room` while mounted; everything inside draws from it. */
export function PresenceRoom({ room: id, token, publishableKey: key, auth, guest, name, relay, mode, look, children }: PresenceRoomProps) {
  const [state, setState] = useState<Ctx>({ room: null, members: [], live: "connecting", error: null });
  // The token (or auth) function may be a fresh closure each render; the
  // room keeps the first and reads the newest through this.
  const cred = key ? auth : token;
  const credRef = useRef(cred);
  credRef.current = cred;
  useEffect(() => {
    let room: Room | null = null;
    let gone = false;
    const c = typeof cred === "function" ? () => (credRef.current as () => Promise<string>)() : cred;
    const how: Partial<ConnectOptions> = key ? (c ? { key, auth: c, name } : { key, guest }) : { token: c };
    connect({ room: id, relay, mode, look, ...how })
      .then((r) => {
        if (gone) return r.leave();
        room = r;
        const sync = () => setState({ room: r, members: r.members, live: r.client.live, error: null });
        r.on("members", sync);
        r.on("live", sync);
        sync();
      })
      .catch((error: Error) => !gone && setState({ room: null, members: [], live: "refused", error }));
    return () => {
      gone = true;
      room?.leave();
    };
    // A different room, relay, key or credential is a different connection.
  }, [id, relay, key, guest, typeof cred === "string" ? cred : cred ? "fn" : ""]);
  return <Presence.Provider value={state}>{children}</Presence.Provider>;
}

/** The room, its members and what you can do in it. */
export function usePresence() {
  const { room, members, live, error } = useContext(Presence);
  // Re-render on the room's changes (camera state, waves).
  const [, tick] = useState(0);
  useEffect(() => room?.client.subscribe(() => tick((n) => n + 1)), [room]);
  return {
    room,
    members,
    live,
    error,
    me: room?.me ?? "",
    waves: room?.client.waves ?? [],
    wave: (sub: string) => room?.wave(sub),
    react: (emoji: string) => room?.react(emoji),
    messages: room?.client.messages ?? ([] as Message[]),
    say: (text: string) => (room ? room.say(text) : Promise.reject(new Error("not in the room yet"))),
    loadEarlier: () => room?.loadEarlier(),
    reactions: room?.client.reactions ?? [],
    answer: (sub: string, yes: boolean) => room?.answer(sub, yes),
    /** Whether this relay has calls (hide call buttons where not). */
    calls: room?.calls ?? false,
    /** Who's ringing you, until answered or over. */
    rings: room?.rings ?? [],
    /** Your call (ringing, joining, on; who's in it, muted), or null. */
    inCall: room?.inCall ?? (null as Call | null),
    call: (sub: string) => (room ? room.call(sub) : Promise.reject(new Error("not in the room yet"))),
    answerCall: (id: string, yes: boolean) => room?.answerCall(id, yes),
    leaveCall: () => room?.leaveCall(),
    setMuted: (muted: boolean) => room?.setMuted(muted),
    setLook: (look: Look) => room?.setLook(look),
    camera: {
      on: room?.camera.on ?? false,
      start: (o?: { mode?: "avatar" | "outline" }) => room?.camera.start(o),
      stop: () => room?.camera.stop(),
    },
  };
}

/** Something drawn into a div of its own from the room, while mounted. */
function useDrawn(draw: (room: Room, el: HTMLDivElement) => () => void, deps: unknown[]) {
  const { room } = useContext(Presence);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => (room && ref.current ? draw(room, ref.current) : undefined), [room, ...deps]);
  return ref;
}

/** A tile per person in the room. */
export function People({ size, self, calls, className }: PeopleOptions & { className?: string }) {
  const ref = useDrawn((room, el) => room.mount(el, { size, self, calls }), [size, self, calls]);
  return <div ref={ref} className={className} />;
}

/** One person's live face (their outline or character). Size the
 * element with CSS; it fills it. */
export function Avatar({ user, size, className, style }: AvatarOptions & { user: string; className?: string; style?: CSSProperties }) {
  const ref = useDrawn((room, el) => room.avatar(el, user, { size }), [user, size]);
  return <div ref={ref} className={className} style={{ width: 36, height: 36, ...style }} />;
}

/** Your camera: on/off, avatar or outline, character and colour. */
export function CameraControls({ controls, className }: CameraOptions & { className?: string }) {
  const ref = useDrawn((room, el) => styled(el, (root) => mountCamera(room, root, { controls })), [controls]);
  return <div ref={ref} className={className} />;
}

/** The call bar: a ring to answer, who you're calling, mute and leave
 * (nothing while there's no call). */
export function CallControls({ className }: { className?: string }) {
  const ref = useDrawn((room, el) => styled(el, (root) => mountCall(room, root)), []);
  return <div ref={ref} className={className} />;
}

/** The room's text: messages, earlier ones a click away, and an input. */
export function Chat({ placeholder, className, style }: ChatOptions & { className?: string; style?: CSSProperties }) {
  const ref = useDrawn((room, el) => room.chat(el, { placeholder }), [placeholder]);
  return <div ref={ref} className={className} style={{ height: 360, ...style }} />;
}

export type { Member, Message, Look, Room, Call };
