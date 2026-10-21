// Files on a host (M7), read-only: the shapes of `crates/proto/src/fs.rs`
// and calls to the shown daemon's `/api/fs/…`. `pane` puts a call on the
// host that block runs on (its VM, say); without it, the daemon's own.

import { OPS, opPath, type OpName } from "./ops";
import type { Client } from "./client";
import type { PaneId } from "../shared/proto";

export type FsKind = "file" | "directory" | "symlink" | "other";

export interface FsEntry {
  name: string;
  path: string;
  type: FsKind;
  size: number;
  mode: number;
  mtime_ms: number;
  /** For a symlink: what it points at, when known. */
  target?: FsKind;
}

export interface FsList {
  path: string;
  parent: string | null;
  entries: FsEntry[];
  truncated: boolean;
}

export const isDir = (e: FsEntry) => e.type === "directory" || e.target === "directory";

async function get<T>(client: Client, op: OpName, q: Record<string, string | number | undefined>): Promise<T> {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) if (v !== undefined) params.set(k, String(v));
  const res = await client.request(OPS[op].method, `${opPath(op)}?${params}`);
  const body = await res.json<T & { error?: string }>().catch(() => null);
  if (!res.ok || body === null) throw new Error(body?.error ?? `HTTP ${res.status}`);
  return body;
}

/** A directory's subdirectories (and links to them). */
export const listDirs = (client: Client, pane: PaneId, path: string) =>
  get<FsList>(client, "fs.list", { pane, path, dirs: 1 });

/** Directories used lately on the block's host, newest first. */
export const recentDirs = (client: Client, pane: PaneId) => get<string[]>(client, "fs.recent", { pane });
