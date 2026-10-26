// Files and images in chat (#107): the composer's chips, the 📎 button, a
// message's pictures, players and file cards, and the viewer a picture opens
// in. Every address is control's own (same-origin, under the page's CSP: a
// `blob:` URL would be blocked, #85), and a file is shown only as its kind
// says it is safe to: pictures, video and audio inline, the rest a card that
// downloads.

import { useEffect, useRef, useState } from "preact/hooks";
import type { ChatFile } from "../proto";
import { kindText, MAX_FILES, pastedName, sizeText, tooBig, type Pending } from "./files";
import { upload } from "./files-upload";

// ---- the composer's side

/** Files chosen for the next message in `room`, uploading as they arrive. */
export function useAttachments(room: string, setError: (e: string | null) => void) {
  const [items, setItems] = useState<Pending[]>([]);
  const aborts = useRef(new Map<string, () => void>());
  const count = useRef(0);
  useEffect(
    () => () => {
      for (const abort of aborts.current.values()) abort();
    },
    [],
  );
  const patch = (key: string, p: Partial<Pending>) => setItems((xs) => xs.map((x) => (x.key === key ? { ...x, ...p } : x)));

  const add = (files: File[]) => {
    const room_left = MAX_FILES - items.length;
    if (files.length > room_left) setError(`A message carries at most ${MAX_FILES} files`);
    for (const f of files.slice(0, Math.max(0, room_left))) {
      const name = pastedName(f);
      const key = `f${++count.current}`;
      const refused = tooBig(f);
      if (refused) {
        setError(`${name}: ${refused}`);
        continue;
      }
      setItems((xs) => [...xs, { key, name, size: f.size, progress: 0, state: "uploading" }]);
      const up = upload(room, f, name, (p) => patch(key, { progress: p }));
      aborts.current.set(key, up.abort);
      up.done.then(
        (file) => patch(key, { state: "done", progress: 1, file }),
        (e: Error) => {
          // A cancel already took its chip away.
          if (e.message !== "Cancelled") patch(key, { state: "failed", error: e.message });
        },
      ).finally(() => aborts.current.delete(key));
    }
  };
  const remove = (key: string) => {
    aborts.current.get(key)?.();
    setItems((xs) => xs.filter((x) => x.key !== key));
  };
  return {
    items,
    add,
    remove,
    /** Ids to post: every file is up (none still going or failed). */
    ids: items.every((x) => x.state === "done") ? items.map((x) => x.file!.id) : null,
    busy: items.some((x) => x.state === "uploading"),
    clear: () => setItems([]),
  };
}

/** Files on a paste or a drop, as a list (a pasted picture is a file). */
export function filesOf(data: DataTransfer | null): File[] {
  return data ? [...data.files] : [];
}

/** Whether a drag carries files, not text or a link. */
export function dragsFiles(e: DragEvent): boolean {
  return !!e.dataTransfer && [...e.dataTransfer.types].includes("Files");
}

/** The 📎 button: opens the system's file picker. */
export function AttachButton({ add }: { add: (files: File[]) => void }) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <input
        ref={input}
        type="file"
        multiple
        hidden
        data-attach-input
        onChange={(e) => {
          add([...(e.currentTarget.files ?? [])]);
          e.currentTarget.value = "";
        }}
      />
      <button type="button" class="compose-tool" title="Attach files" aria-label="Attach files" data-attach-button onClick={() => input.current?.click()}>
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
          <path d="M13.2 7.6l-5 5a3.2 3.2 0 0 1-4.5-4.5l5.3-5.3a2.1 2.1 0 0 1 3 3L6.6 11a1 1 0 0 1-1.5-1.5l4.7-4.7" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
    </>
  );
}

/** The chips under the box: each file with its progress and a remove. */
export function AttachChips({ items, remove }: { items: Pending[]; remove: (key: string) => void }) {
  if (!items.length) return null;
  return (
    <ul class="attach-chips" aria-label="Files to send">
      {items.map((x) => (
        <li key={x.key} class={`attach-chip ${x.state}`} data-attach-chip={x.name} title={x.error ?? x.name}>
          {x.file?.kind === "image" ? <img src={x.file.url} alt="" width="28" height="28" /> : <span class="attach-kind">{kindText({ name: x.name, mime: x.file?.mime ?? "" })}</span>}
          <span class="attach-name">{x.name}</span>
          <span class="attach-size">{x.state === "failed" ? x.error : sizeText(x.size)}</span>
          {x.state === "uploading" && <progress max="1" value={x.progress} aria-label={`Uploading ${x.name}`} />}
          <button type="button" title="Remove" aria-label={`Remove ${x.name}`} data-attach-remove onClick={() => remove(x.key)}>
            ✕
          </button>
        </li>
      ))}
    </ul>
  );
}

// ---- a message's side

/** A message's files: pictures sized to fit (they open full size), players
 * for video and audio, a card for the rest. */
export function MessageFiles({ files }: { files: ChatFile[] }) {
  const [open, setOpen] = useState<ChatFile | null>(null);
  if (!files.length) return null;
  return (
    <div class="msg-files" data-msg-files>
      {files.map((f) => {
        switch (f.kind) {
          case "image":
            return (
              <button key={f.id} type="button" class="msg-image" title={f.name} aria-label={`Open ${f.name}`} data-msg-image onClick={() => setOpen(f)}>
                <img
                  src={f.url}
                  alt={f.name}
                  loading="lazy"
                  width={f.width ?? undefined}
                  height={f.height ?? undefined}
                  style={pictureSize(f)}
                />
              </button>
            );
          case "video":
            return (
              <figure key={f.id} class="msg-media">
                <video src={f.url} controls preload="metadata" playsInline aria-label={f.name} />
                <figcaption>
                  {f.name} · {sizeText(f.size)}
                </figcaption>
              </figure>
            );
          case "audio":
            return (
              <figure key={f.id} class="msg-media audio">
                <audio src={f.url} controls preload="metadata" aria-label={f.name} />
                <figcaption>
                  {f.name} · {sizeText(f.size)}
                </figcaption>
              </figure>
            );
          default:
            return <FileCard key={f.id} f={f} />;
        }
      })}
      {open && <Viewer f={open} close={() => setOpen(null)} />}
    </div>
  );
}

/** The picture's size in the room: its own, shrunk to fit 360 by 320, so
 * the frame is the picture's (UI audit #156: CSS alone sized the frame to
 * the picture's full width, a dark strip beside it) and is there before it
 * loads. A narrower room shrinks it further (`max-width`). */
const PIC_W = 360;
const PIC_H = 320;
function pictureSize(f: ChatFile): { width: string; aspectRatio: string } | undefined {
  if (!f.width || !f.height) return undefined;
  const k = Math.min(1, PIC_W / f.width, PIC_H / f.height);
  return { width: `${Math.max(1, Math.round(f.width * k))}px`, aspectRatio: `${f.width} / ${f.height}` };
}

function FileCard({ f }: { f: ChatFile }) {
  return (
    <div class="msg-file" data-msg-file={f.name}>
      <span class="attach-kind" aria-hidden="true">
        {kindText(f)}
      </span>
      <span class="msg-file-text">
        <b title={f.name}>{f.name}</b>
        <span>
          {sizeText(f.size)} · {kindText(f)}
        </span>
      </span>
      <a class="msg-file-get" href={`${f.url}?download=1`} download={f.name} title="Download" aria-label={`Download ${f.name}`} data-file-download>
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M8 2v8M4.5 7L8 10.5 11.5 7M3 13.5h10" />
        </svg>
      </a>
    </div>
  );
}

/** A picture full size over the page; Escape or a click outside closes it. */
function Viewer({ f, close }: { f: ChatFile; close: () => void }) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, []);
  return (
    <div class="image-viewer" role="dialog" aria-modal="true" aria-label={f.name} data-image-viewer onClick={close}>
      <img src={f.url} alt={f.name} onClick={(e) => e.stopPropagation()} />
      <div class="image-viewer-bar" onClick={(e) => e.stopPropagation()}>
        <span>{f.name}</span>
        <a href={`${f.url}?download=1`} download={f.name}>
          Download
        </a>
        <button type="button" aria-label="Close" data-viewer-close onClick={close}>
          ✕
        </button>
      </div>
    </div>
  );
}
