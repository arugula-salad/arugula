// @arugula/presence: join a room and see who's around as live avatars or
// outlines (arugula.video, Arugula S43/S44). A PresenceClient over a
// transport: arugula.video's Phoenix relay (../../PROTOCOL.md), or Arugula
// control's.

export { PresenceClient, REACT_MS, REACTIONS, MESSAGES_KEPT, mergeMessages, type PresenceClientOptions, type Seen, type Mine } from "./client.ts";
export type { Transport, TransportEvents, Member, Message, Mode, State } from "./transport.ts";
export { arugulaTransport, type ArugulaTransportOptions } from "./arugula.ts";
export { phoenixTransport, socketParams, unprefix, memberOf, type PhoenixTransportOptions, type TokenAuth, type KeyAuth, type MemberAuth } from "./phoenix.ts";
