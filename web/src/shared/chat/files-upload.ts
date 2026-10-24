// Sending a file to a room (#107): one `POST /api/chat/rooms/{id}/files` per
// file, by XMLHttpRequest because only it reports the bytes sent.

import type { ChatFile } from "../proto";
import { refusalOf } from "../maintenance";

/** Upload `file` to `room`, reporting progress as 0 to 1. `abort()` cancels. */
export function upload(room: string, file: File, name: string, progress: (p: number) => void): { done: Promise<ChatFile>; abort: () => void } {
  const xhr = new XMLHttpRequest();
  const done = new Promise<ChatFile>((resolve, reject) => {
    xhr.open("POST", `/api/chat/rooms/${encodeURIComponent(room)}/files?name=${encodeURIComponent(name)}`);
    xhr.withCredentials = true;
    xhr.responseType = "json";
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) progress(e.loaded / e.total);
    };
    xhr.onload = () => {
      const body = (xhr.response ?? {}) as ChatFile & { error?: string; reason?: string };
      if (xhr.status >= 200 && xhr.status < 300) resolve(body);
      else reject(new Error(refusalOf(xhr.status, body, `Upload failed (${xhr.status})`)));
    };
    xhr.onerror = () => reject(new Error("Couldn't reach control to upload"));
    xhr.onabort = () => reject(new Error("Cancelled"));
    xhr.send(file);
  });
  return { done, abort: () => xhr.abort() };
}
