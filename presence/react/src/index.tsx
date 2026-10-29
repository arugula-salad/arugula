// @arugula/presence-react: arugula.stream for React (../../sdk/API.md).
// <PresenceRoom> joins a room with connect() and shares it through
// context; <People>, <CameraControls> and <Avatar> draw into elements of
// their own with the same code the custom elements use; usePresence()
// gives the room's members and actions to your components.

import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { connect, styled, type ConnectOptions, type Room } from "@arugula/presence-elements/room";
import { mountCamera, type CameraOptions } from "@arugula/presence-elements/controls";
import type { AvatarOptions, PeopleOptions } from "@arugula/presence-elements/view";
import type { Member } from "@arugula/presence";
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
  token: ConnectOptions["token"];
  relay?: string;
  mode?: "avatar" | "outline";
  look?: Look;
  children?: ReactNode;
}

/** Join `room` while mounted; everything inside draws from it. */
export function PresenceRoom({ room: id, token, relay, mode, look, children }: PresenceRoomProps) {
  const [state, setState] = useState<Ctx>({ room: null, members: [], live: "connecting", error: null });
  // The token function may be a fresh closure each render; the room keeps
  // the first and reads the newest through this.
  const tokenRef = useRef(token);
  tokenRef.current = token;
  useEffect(() => {
    let room: Room | null = null;
    let gone = false;
    const t = typeof token === "string" ? token : () => (tokenRef.current as () => Promise<string>)();
    connect({ room: id, token: t, relay, mode, look })
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
    // A different room, relay or token kind is a different connection.
  }, [id, relay, typeof token === "string" ? token : "fn"]);
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
    answer: (sub: string, yes: boolean) => room?.answer(sub, yes),
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
export function People({ size, self, className }: PeopleOptions & { className?: string }) {
  const ref = useDrawn((room, el) => room.mount(el, { size, self }), [size, self]);
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

export type { Member, Look, Room };
