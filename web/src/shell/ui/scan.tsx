// Scanning a QR code with the camera (#761): the code a waiting browser or
// phone shows, read on the device that approves it. A phone's own camera
// app reads it too (it's a link, `#device=…`); this is for the page that's
// already open, the app's window, or a phone's home-screen app, whose
// storage a link opened from the camera wouldn't reach.
//
// The browser's own BarcodeDetector where there is one (Chrome, Android);
// elsewhere (Safari, Firefox) jsQR, loaded only then.

import { useEffect, useRef, useState } from "preact/hooks";

interface Detector {
  detect(source: HTMLVideoElement): Promise<{ rawValue: string }[]>;
}

declare global {
  interface Window {
    BarcodeDetector?: new (o: { formats: string[] }) => Detector;
  }
}

/** Whether this browser can try: a camera API, in a secure context. */
export const canScan = () => !!navigator.mediaDevices?.getUserMedia && window.isSecureContext;

/** A decoder for the video's current frame: the text of a QR code in it,
 * or null. */
async function decoder(video: HTMLVideoElement): Promise<() => Promise<string | null>> {
  const Native = window.BarcodeDetector;
  if (Native) {
    try {
      const d = new Native({ formats: ["qr_code"] });
      return async () => (await d.detect(video))[0]?.rawValue ?? null;
    } catch {
      // No QR support in this one: jsQR below.
    }
  }
  const { default: jsQR } = await import("jsqr");
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  return async () => {
    const w = video.videoWidth;
    const h = video.videoHeight;
    if (!ctx || !w || !h) return null;
    canvas.width = w;
    canvas.height = h;
    ctx.drawImage(video, 0, 0, w, h);
    return jsQR(ctx.getImageData(0, 0, w, h).data, w, h, { inversionAttempts: "dontInvert" })?.data ?? null;
  };
}

/** The camera, looking for a QR code `accept` takes; it stops once one
 * does, or on Cancel. */
export function Scanner({ accept, close }: { accept: (text: string) => boolean; close: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const [err, setErr] = useState("");
  // The latest, for the loop that started with the first.
  const took = useRef({ accept, close });
  took.current = { accept, close };
  useEffect(() => {
    let stream: MediaStream | null = null;
    let stopped = false;
    let timer: number | undefined;
    void (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
        // Closed while the browser asked: the camera goes off again.
        if (stopped || !video.current) return stream.getTracks().forEach((t) => t.stop());
        video.current.srcObject = stream;
        await video.current.play();
        const read = await decoder(video.current);
        const tick = async () => {
          if (stopped) return;
          const text = await read().catch(() => null);
          if (stopped) return;
          if (text && took.current.accept(text)) return took.current.close();
          timer = window.setTimeout(() => void tick(), 200);
        };
        void tick();
      } catch (e) {
        setErr((e as Error).name === "NotAllowedError" ? "The camera isn't allowed here. Type the code instead." : `No camera: ${(e as Error).message}`);
      }
    })();
    return () => {
      stopped = true;
      clearTimeout(timer);
      for (const t of stream?.getTracks() ?? []) t.stop();
    };
  }, []);
  return (
    <div class="control-scan" data-scanner>
      {err ? <p class="control-error">{err}</p> : <video ref={video} class="control-scan-video" muted playsInline />}
      <button type="button" onClick={close}>
        {err ? "Close" : "Cancel"}
      </button>
    </div>
  );
}
