// Files and images in chat (#107): choosing them, uploading each with its
// progress, and the words for a file's size and kind. A file goes up as soon
// as it's chosen (`POST /api/chat/rooms/{id}/files?name=`, the bytes the
// body), is kept for its uploader, and a message is posted with the ids
// (`ChatPost.files`). The limits are control's (`MAX_FILE_BYTES`,
// `MAX_FILES`); they are checked here only to refuse early.

import type { ChatFile } from "../proto";

export const MAX_FILE_BYTES = 10 * 1024 * 1024;
export const MAX_FILES = 10;

/** One file on its way into a message. */
export interface Pending {
  key: string;
  name: string;
  size: number;
  /** 0 to 1 of the bytes sent. */
  progress: number;
  state: "uploading" | "done" | "failed";
  /** Control's row for it, once it's up. */
  file?: ChatFile;
  error?: string;
}

/** `1.4 MB`, `812 KB`, `37 B`. */
export function sizeText(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  const mb = n / (1024 * 1024);
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
}

/** The short word on a file's card: its extension, else its type. */
export function kindText(f: Pick<ChatFile, "name" | "mime">): string {
  const ext = /\.([A-Za-z0-9]{1,5})$/.exec(f.name)?.[1];
  if (ext) return ext.toUpperCase();
  const sub = f.mime.split("/")[1];
  return f.mime === "application/octet-stream" || !sub ? "FILE" : sub.toUpperCase().slice(0, 5);
}

/** A pasted picture has a name like "image.png" (or none); give it one that
 * says when, so several don't all read the same. */
export function pastedName(f: File, now = new Date()): string {
  if (f.name && !/^image\.\w+$/i.test(f.name)) return f.name;
  const ext = /^image\/(png|jpeg|gif|webp)$/.exec(f.type)?.[1]?.replace("jpeg", "jpg") ?? "png";
  const p = (n: number) => String(n).padStart(2, "0");
  return `pasted-${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}.${ext}`;
}

/** Why `f` can't be sent at all, or null. */
export function tooBig(f: { size: number }): string | null {
  if (f.size === 0) return "That file is empty";
  if (f.size > MAX_FILE_BYTES) return `A file is at most ${sizeText(MAX_FILE_BYTES)}`;
  return null;
}
