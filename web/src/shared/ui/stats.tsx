// Stat tiles in a Doc (#254): a ```stats fence holding a JSON list of 1 to
// 6 tiles, each a label and a value, with an optional delta and its tone.
// The rules are the daemon's (`check_stats` in
// `crates/daemon/src/docs/embeds.rs`) and say the same things; a spec runs
// both over the same files (`tests/fixtures/docs/`), as charts do. A delta
// always carries an arrow, so its tone is never colour alone.

export const MAX_TILES = 6;
const TEXT: [keyof Tile, number][] = [
  ["label", 40],
  ["value", 24],
  ["delta", 16],
];
const KEYS = ["label", "value", "delta", "tone"];
const TONES = ["up", "down", "flat"];
const ARROW = { up: "↑", down: "↓", flat: "→" } as const;

export interface Tile {
  label: string;
  value: string;
  delta?: string;
  tone?: "up" | "down" | "flat";
}

const oneLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

/** Null if the tiles can be drawn, else one line saying why not. */
export function checkTiles(tiles: unknown): string | null {
  if (!Array.isArray(tiles)) return `must be a JSON list of 1 to ${MAX_TILES} tiles`;
  if (tiles.length < 1 || tiles.length > MAX_TILES) return `${tiles.length} tiles; a stats fence holds 1 to ${MAX_TILES}`;
  for (const [i, tile] of tiles.entries()) {
    const n = i + 1;
    if (!isObject(tile)) return `tile ${n} must be an object`;
    const unknown = Object.keys(tile).find((k) => !KEYS.includes(k));
    if (unknown !== undefined) return `tile ${n}: unknown key ${JSON.stringify(unknown)}; a tile has label, value, delta and tone`;
    for (const [key, max] of TEXT) {
      const v = tile[key];
      if (v === undefined) {
        if (key === "delta") continue;
        return `tile ${n} has no ${key}`;
      }
      if (typeof v !== "string" || !v.trim() || [...v].length > max) {
        return `tile ${n}: ${key} must be text ("1,284", not 1284), at most ${max} characters`;
      }
    }
    if (tile.tone === undefined) continue;
    if (tile.delta === undefined) return `tile ${n}: tone needs a delta`;
    if (typeof tile.tone !== "string" || !TONES.includes(tile.tone)) {
      return `tile ${n}: tone must be "up", "down" or "flat", got ${oneLine(JSON.stringify(tile.tone))}`;
    }
  }
  return null;
}

/** A stats fence's source, parsed, or one line saying why it can't be:
 * `stats: …`, as the daemon's refusal says it. */
export function parseStats(source: string): { tiles: Tile[] } | { why: string } {
  let tiles: unknown;
  try {
    tiles = JSON.parse(source);
  } catch (e) {
    return { why: `stats: not JSON (${oneLine((e as Error).message)})` };
  }
  const why = checkTiles(tiles);
  return why ? { why: `stats: ${why}` } : { tiles: tiles as Tile[] };
}

/** Null if the tiles can be drawn, else why not. */
export function checkStats(source: string): string | null {
  const p = parseStats(source);
  return "why" in p ? p.why : null;
}

/** `[stats: Sessions 1,284 (+12%); Errors 3]`; `[stats]` for tiles that
 * can't be read: what a Doc's text has in place of the fence. */
export function statsSummary(source: string): string {
  const p = parseStats(source);
  if ("why" in p) return "[stats]";
  const tiles = p.tiles.map((t) => `${oneLine(t.label)} ${oneLine(t.value)}${t.delta === undefined ? "" : ` (${oneLine(t.delta)})`}`);
  return `[stats: ${tiles.join("; ")}]`;
}

/** A delta's tone: the one given, else its sign's. */
export const toneOf = (t: Tile): "up" | "down" | "flat" => t.tone ?? (/^\+/.test(t.delta ?? "") ? "up" : /^[-−]/.test(t.delta ?? "") ? "down" : "flat");

/** A stats fence: the tiles, or why they aren't drawn and the source. */
export function Stats({ source }: { source: string }) {
  const p = parseStats(source);
  if ("why" in p) {
    return (
      <div class="chart-note" data-chart-note>
        <p class="dim">Stats not drawn: {p.why.replace(/^stats: /, "")}</p>
        <pre>{source}</pre>
      </div>
    );
  }
  return (
    <div class="doc-stats-box">
      <dl class="doc-stats" data-tiles={p.tiles.length}>
        {p.tiles.map((t, i) => {
          const tone = toneOf(t);
          return (
            <div key={i} class="doc-stat">
              <dt>{t.label}</dt>
              <dd class="doc-stat-value">{t.value}</dd>
              {t.delta !== undefined && (
                <dd class={`doc-stat-delta ${tone}`} data-tone={tone}>
                  <span role="img" aria-label={tone}>
                    {ARROW[tone]}
                  </span>{" "}
                  {t.delta}
                </dd>
              )}
            </div>
          );
        })}
      </dl>
    </div>
  );
}
