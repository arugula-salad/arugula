// What a member sees of the app (plan §3.11, a seam for Product mode): the
// rail's places, whether the panel has a terminal and the floor, how much
// of a transcript shows, and whether the composer starts work or asks for
// it. It always answers the engineer's set; the rail reads it now, the
// panel and the transcript from Lift 4.

export type Place = "overview" | "boards" | "docs" | "machines" | "chat";

export interface Surfaces {
  rail: Place[];
  terminal: boolean;
  floor: boolean;
  transcript: "full" | "said";
  composer: "start" | "request";
  /** Agents by name (`claude@dust`) or by role (Lead, Worker). */
  names: "agent" | "role";
}

const ENGINEER: Surfaces = { rail: ["overview", "boards", "docs", "machines", "chat"], terminal: true, floor: true, transcript: "full", composer: "start", names: "agent" };

/** What `member` sees in `project`: today, everyone the engineer's set. */
export function surfacesFor(_member?: string | null, _project?: string | null): Surfaces {
  return ENGINEER;
}
