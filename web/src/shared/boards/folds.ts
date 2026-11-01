// Which headings of a board a viewer has folded (#480): a group's on a board
// with groups, a lane's on one without. Kept per board in this browser's
// localStorage, never shared. A fold the viewer never made or undid falls
// back to the default: everything open.

/** Whether the Done lane starts folded, on a board without groups. The one line to flip. */
export const FOLD_DONE_BY_DEFAULT = false;

/** Fold key to whether that heading is folded; a key not here is at its default. */
export type Folds = Record<string, boolean>;

const storeKey = (board: string) => `arugula.board.folds.${board}`;

/** The fold key of a group's heading; `null` is the rows with no group. */
export const groupFold = (group: string | null) => `group:${group ?? "none"}`;

/** The fold key of a lane's heading, by its zone's id. */
export const laneFold = (zone: string) => `lane:${zone}`;

/** Whether the heading with this key is folded. */
export function isFolded(folds: Folds, key: string): boolean {
  return folds[key] ?? (FOLD_DONE_BY_DEFAULT && key === laneFold("lane_done"));
}

/** What this browser kept for the board; nothing when it kept none or storage is off. */
export function loadFolds(board: string): Folds {
  try {
    const v = JSON.parse(localStorage.getItem(storeKey(board)) ?? "{}") as unknown;
    if (!v || typeof v !== "object") return {};
    return Object.fromEntries(Object.entries(v).filter(([, on]) => typeof on === "boolean"));
  } catch {
    return {};
  }
}

export function saveFolds(board: string, folds: Folds) {
  try {
    localStorage.setItem(storeKey(board), JSON.stringify(folds));
  } catch {
    // A private window: folded for as long as the page is open.
  }
}
