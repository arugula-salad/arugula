// A call's audio and video, on LiveKit (PROTOCOL.md "Calls"; #318's P7
// voice, P8 video): the relay rings and hands this page a token for one
// LiveKit room, and this joins it with the microphone on, plays everyone
// else, and says who's there, who's muted, who's speaking and whose video
// is on. livekit-client loads only when a call starts, so a page that
// never calls doesn't pay for it.
//
// Video is off until you turn it on, and it comes from the presence
// camera: the client hands setVideo the camera's own track, and this
// publishes a clone of it (one device, opened once; the rig keeps reading
// the original). Others' video goes only to the <video> elements a view
// attaches, and adaptive stream sends each one just the layer its size
// needs, and nothing while it's out of sight.

import type { LocalVideoTrack, RemoteVideoTrack } from "livekit-client";
import type { CallJoin } from "./transport.ts";

/** Someone in the call, you included. */
export interface CallPerson {
  sub: string;
  name: string;
  muted: boolean;
  speaking: boolean;
  /** Their audio reaches this page (yours: your microphone is published). */
  audio: boolean;
  /** Their camera's video is on in the call and reaches this page (yours:
   * your video is published). */
  video: boolean;
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
  /** Publish a clone of `camera` (the presence camera's track) as your
   * video, in place of any before; null takes your video down. */
  setVideo(camera: MediaStreamTrack | null): Promise<void>;
  /** Show `sub`'s video (yours too) in `el`, now and whenever it comes
   * back; returns a function to stop. */
  attach(sub: string, el: HTMLVideoElement): () => void;
  /** From a click, if the browser blocked the call's audio. */
  startAudio(): Promise<void>;
  leave(): Promise<void>;
}

/** Joins a call's LiveKit room (a test passes a fake). */
export type JoinCall = (j: CallJoin, on: LiveCallEvents) => Promise<LiveCall>;

/** A call's video at the top: 640×360 at 24 fps, half a megabit. With
 * simulcast, a 320×180 layer goes up beside it for small tiles. */
export const VIDEO = { width: 640, height: 360, fps: 24, maxBitrate: 500_000 };

export const joinLiveKit: JoinCall = async (j, on) => {
  const { Room, RoomEvent, Track, DisconnectReason, VideoPresets } = await import("livekit-client");
  const room = new Room({
    // Each video element gets the layer its size needs, and none while
    // it's hidden or off screen; a publisher stops encoding a layer
    // nobody's watching.
    adaptiveStream: true,
    dynacast: true,
    publishDefaults: {
      simulcast: true,
      videoEncoding: { maxBitrate: VIDEO.maxBitrate, maxFramerate: VIDEO.fps },
      videoSimulcastLayers: [VideoPresets.h180],
    },
  });
  // Everyone else's audio plays from elements of its own, out of sight.
  const players = new Set<HTMLMediaElement>();
  // The video elements views attached, by whose video they show.
  const views = new Map<string, Set<HTMLVideoElement>>();
  const me = room.localParticipant;
  // `sub`'s video now: their newest camera track that's on (a republished
  // camera, after a mode change, briefly has two).
  const camOf = (sub: string) => {
    const p = sub === me.identity ? me : room.getParticipantByIdentity(sub);
    const cams = [...(p?.trackPublications.values() ?? [])].filter((x) => x.source === Track.Source.Camera && x.videoTrack && !x.isMuted);
    return cams[cams.length - 1]?.videoTrack;
  };
  // Which track each view shows, so a change re-attaches and nothing else.
  const showing = new WeakMap<HTMLVideoElement, LocalVideoTrack | RemoteVideoTrack>();
  // Into every view of `sub`, their video now (or nothing).
  const show = (sub: string) => {
    const t = camOf(sub);
    for (const el of views.get(sub) ?? []) {
      const was = showing.get(el);
      if (was === t) continue;
      was?.detach(el);
      if (t) {
        t.attach(el);
        showing.set(el, t);
      } else {
        el.srcObject = null;
        showing.delete(el);
      }
    }
  };
  // The clone this page publishes, stopped when it's taken down.
  let mine: MediaStreamTrack | null = null;
  const tell = () => {
    for (const sub of views.keys()) show(sub);
    const speaking = new Set(room.activeSpeakers.map((p) => p.identity));
    on.people([
      {
        sub: me.identity,
        name: me.name || me.identity,
        muted: !me.isMicrophoneEnabled,
        speaking: speaking.has(me.identity),
        audio: !!me.getTrackPublication(Track.Source.Microphone)?.track,
        video: !!me.getTrackPublication(Track.Source.Camera)?.track,
      },
      ...[...room.remoteParticipants.values()].map((p) => {
        const mic = p.getTrackPublication(Track.Source.Microphone);
        const cam = p.getTrackPublication(Track.Source.Camera);
        return {
          sub: p.identity,
          name: p.name || p.identity,
          muted: !mic || mic.isMuted,
          speaking: speaking.has(p.identity),
          audio: !!mic?.isSubscribed && !!mic.track,
          video: !!cam?.isSubscribed && !!cam.track && !cam.isMuted,
        };
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
      // Audio's elements go; video's views are the page's, and tell()
      // shows them whatever's on now (or blanks them).
      if (track.kind === Track.Kind.Audio) {
        for (const el of track.detach()) {
          el.remove();
          players.delete(el);
        }
      }
      tell();
    })
    .on(RoomEvent.LocalTrackPublished, tell)
    .on(RoomEvent.LocalTrackUnpublished, tell)
    .on(RoomEvent.ParticipantConnected, tell)
    .on(RoomEvent.ParticipantDisconnected, tell)
    .on(RoomEvent.TrackPublished, tell)
    .on(RoomEvent.TrackUnpublished, tell)
    .on(RoomEvent.TrackMuted, tell)
    .on(RoomEvent.TrackUnmuted, tell)
    .on(RoomEvent.ActiveSpeakersChanged, tell)
    .on(RoomEvent.AudioPlaybackStatusChanged, () => on.blocked?.(!room.canPlaybackAudio))
    .on(RoomEvent.Reconnecting, () => on.state("reconnecting"))
    .on(RoomEvent.Reconnected, () => on.state("on"))
    .on(RoomEvent.Disconnected, (why) => {
      for (const el of players) el.remove();
      players.clear();
      for (const els of views.values())
        for (const el of els) {
          showing.get(el)?.detach(el);
          showing.delete(el);
          el.srcObject = null;
        }
      mine?.stop();
      mine = null;
      on.state("ended", why === DisconnectReason.CLIENT_INITIATED);
    });
  await room.connect(j.url, j.token);
  on.state("on");
  try {
    await me.setMicrophoneEnabled(true);
  } catch {
    // No microphone, or not allowed: in the call, listening, shown muted.
  }
  tell();
  const takeDown = async () => {
    const pub = me.getTrackPublication(Track.Source.Camera);
    if (pub?.track) await me.unpublishTrack(pub.track, true);
    mine?.stop();
    mine = null;
  };
  return {
    setMuted: async (muted) => {
      await me.setMicrophoneEnabled(!muted);
      tell();
    },
    setVideo: async (camera) => {
      await takeDown();
      if (camera) {
        // The same device, not a second getUserMedia: a clone shares the
        // camera's capture, and the browser crops and scales it to its own
        // size and rate. The outline camera asks for 15 fps; raised for a
        // call (it still traces at its own few a second).
        if ((camera.getSettings().frameRate ?? VIDEO.fps) < VIDEO.fps) await camera.applyConstraints({ ...camera.getConstraints(), frameRate: { ideal: VIDEO.fps } }).catch(() => {});
        const copy = camera.clone();
        await copy.applyConstraints({ width: { ideal: VIDEO.width }, height: { ideal: VIDEO.height }, frameRate: { ideal: VIDEO.fps } }).catch(() => {});
        mine = copy;
        try {
          await me.publishTrack(copy, { source: Track.Source.Camera, name: "camera" });
        } catch (e) {
          copy.stop();
          mine = null;
          throw e;
        }
      }
      tell();
    },
    attach: (sub, el) => {
      let set = views.get(sub);
      if (!set) views.set(sub, (set = new Set()));
      set.add(el);
      show(sub);
      return () => {
        set.delete(el);
        showing.get(el)?.detach(el);
        showing.delete(el);
        el.srcObject = null;
      };
    },
    startAudio: () => room.startAudio(),
    leave: () => room.disconnect(),
  };
};
