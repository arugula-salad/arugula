// A call's audio, on LiveKit (PROTOCOL.md "Calls"; #318's P7): the relay
// rings and hands this page a token for one LiveKit room, and this joins
// it with the microphone on, plays everyone else, and says who's there,
// who's muted and who's speaking. livekit-client loads only when a call
// starts, so a page that never calls doesn't pay for it. Audio only.

import type { CallJoin } from "./transport.ts";

/** Someone in the call, you included. */
export interface CallPerson {
  sub: string;
  name: string;
  muted: boolean;
  speaking: boolean;
  /** Their audio reaches this page (yours: your microphone is published). */
  audio: boolean;
}

export interface LiveCallEvents {
  people(people: CallPerson[]): void;
  /** Connected, reconnecting by itself, or gone (`byUs`: we left). */
  state(state: "on" | "reconnecting" | "ended", byUs?: boolean): void;
  /** The browser blocked playing the call (no click yet): `startAudio`
   * from a click plays it. */
  blocked?(blocked: boolean): void;
}

export interface LiveCall {
  setMuted(muted: boolean): Promise<void>;
  /** From a click, if the browser blocked the call's audio. */
  startAudio(): Promise<void>;
  leave(): Promise<void>;
}

/** Joins a call's LiveKit room (a test passes a fake). */
export type JoinCall = (j: CallJoin, on: LiveCallEvents) => Promise<LiveCall>;

export const joinLiveKit: JoinCall = async (j, on) => {
  const { Room, RoomEvent, Track, DisconnectReason } = await import("livekit-client");
  const room = new Room();
  // Everyone else's audio plays from elements of its own, out of sight.
  const players = new Set<HTMLMediaElement>();
  const tell = () => {
    const speaking = new Set(room.activeSpeakers.map((p) => p.identity));
    const me = room.localParticipant;
    on.people([
      { sub: me.identity, name: me.name || me.identity, muted: !me.isMicrophoneEnabled, speaking: speaking.has(me.identity), audio: !!me.getTrackPublication(Track.Source.Microphone)?.track },
      ...[...room.remoteParticipants.values()].map((p) => {
        const mic = p.getTrackPublication(Track.Source.Microphone);
        return { sub: p.identity, name: p.name || p.identity, muted: !mic || mic.isMuted, speaking: speaking.has(p.identity), audio: !!mic?.isSubscribed && !!mic.track };
      }),
    ]);
  };
  room
    .on(RoomEvent.TrackSubscribed, (track) => {
      if (track.kind === Track.Kind.Audio) {
        const el = track.attach();
        el.hidden = true;
        document.body.append(el);
        players.add(el);
      }
      tell();
    })
    .on(RoomEvent.TrackUnsubscribed, (track) => {
      for (const el of track.detach()) {
        el.remove();
        players.delete(el);
      }
      tell();
    })
    .on(RoomEvent.ParticipantConnected, tell)
    .on(RoomEvent.ParticipantDisconnected, tell)
    .on(RoomEvent.TrackMuted, tell)
    .on(RoomEvent.TrackUnmuted, tell)
    .on(RoomEvent.LocalTrackPublished, tell)
    .on(RoomEvent.ActiveSpeakersChanged, tell)
    .on(RoomEvent.AudioPlaybackStatusChanged, () => on.blocked?.(!room.canPlaybackAudio))
    .on(RoomEvent.Reconnecting, () => on.state("reconnecting"))
    .on(RoomEvent.Reconnected, () => on.state("on"))
    .on(RoomEvent.Disconnected, (why) => {
      for (const el of players) el.remove();
      players.clear();
      on.state("ended", why === DisconnectReason.CLIENT_INITIATED);
    });
  await room.connect(j.url, j.token);
  on.state("on");
  try {
    await room.localParticipant.setMicrophoneEnabled(true);
  } catch {
    // No microphone, or not allowed: in the call, listening, shown muted.
  }
  tell();
  return {
    setMuted: async (muted) => {
      await room.localParticipant.setMicrophoneEnabled(!muted);
      tell();
    },
    startAudio: () => room.startAudio(),
    leave: () => room.disconnect(),
  };
};
