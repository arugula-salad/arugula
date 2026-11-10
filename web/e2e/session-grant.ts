// A project's standing grant, written straight into control's database for the
// specs that start agent sessions with no box behind them: sessions run under
// one ("One standing grant per project starts runs", aru-099). A v1, as the
// platform still keeps from before passkeys, signed by nobody: only a box
// checks the signature, and no box runs in these specs.

import { randomBytes } from "node:crypto";
import type { DatabaseSync } from "node:sqlite";

/** `project`, of `team`, gets a live standing grant, good for a day. */
export function allowSessions(db: DatabaseSync, project: string, team: string) {
  const id = randomBytes(16).toString("hex");
  const delegation = "e2e-delegation";
  const made = Date.now() - 1000;
  const expires = made + 86_400_000;
  const body = [
    "arugula project grant v1",
    `id ${id}`,
    `project ${project}`,
    `owner team:${team}`,
    `delegation ${delegation}`,
    "resource hosted",
    "to platform",
    "caps runs.instruct",
    "caps runs.start",
    `made ${made}`,
    `expires ${expires}`,
    "by e2e",
    "",
  ].join("\n");
  db.prepare(
    `INSERT INTO run_grants (id, project_id, delegation, body, sig, by_device, made_at, expires_at, created_at)
     VALUES (?, ?, ?, ?, 'unsigned', 'e2e', ?, ?, ?)`,
  ).run(id, project, delegation, body, made, expires, made);
}
