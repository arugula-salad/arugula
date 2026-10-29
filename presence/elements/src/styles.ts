// The elements' own stylesheet (in each shadow root): dark by default,
// like arugula.stream, and themed from the page with --arugula-* custom
// properties on any ancestor. The characters and outlines read the
// theme's variables (--accent, --fg, …), defined here from those.

export const CSS = `
:host {
  --accent: var(--arugula-accent, #4fd1c5);
  --teal: var(--arugula-teal, #2dd4bf);
  --ok: var(--arugula-ok, #86efac);
  --warn: var(--arugula-warn, #f5b74e);
  --info: var(--arugula-info, #7cb4ff);
  --danger: var(--arugula-danger, #f87171);
  --fg: var(--arugula-fg, #e6e9ef);
  --dim: var(--arugula-dim, #7a8496);
  --void: var(--arugula-bg, #06080d);
  --bg: var(--arugula-bg, #06080d);
  --surface: var(--arugula-surface, color-mix(in srgb, var(--fg) 5%, var(--void)));
  --line: var(--arugula-line, color-mix(in srgb, var(--fg) 12%, var(--void)));
  --radius: var(--arugula-radius, 12px);
  display: block;
  color: var(--fg);
  font: 14px/1.4 var(--arugula-font, system-ui, sans-serif);
}
:host([hidden]) { display: none; }
* { box-sizing: border-box; }
.arugula-people { display: grid; gap: 10px; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); }
.arugula-people.head { grid-template-columns: repeat(auto-fill, minmax(72px, 1fr)); gap: 8px; }
.tile { margin: 0; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); overflow: hidden; }
.tile.live { border-color: color-mix(in srgb, var(--accent) 45%, var(--line)); }
.tile.away { opacity: 0.6; }
.view { position: relative; aspect-ratio: 4 / 3; background: var(--void); color: var(--accent); }
.head .view { aspect-ratio: 1; }
.view canvas, .arugula-avatar canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; }
.initial { position: absolute; inset: 0; display: grid; place-items: center; color: var(--dim); font-weight: 700; font-size: 2em; }
figcaption { display: flex; align-items: center; gap: 6px; padding: 6px 10px; border-top: 1px solid var(--line); font-size: 13px; }
.head figcaption { padding: 4px 6px; font-size: 11px; }
.name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tag { font: 10px ui-monospace, monospace; text-transform: uppercase; letter-spacing: 0.06em; color: var(--accent); }
.head .tag { display: none; }
.arugula-avatar { position: relative; width: 100%; height: 100%; min-width: 24px; min-height: 24px; background: var(--void); border-radius: var(--radius); overflow: hidden; color: var(--accent); }
.arugula-avatar .initial { font-size: 1em; }
.arugula-camera { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; font-size: 13px; }
.seg { display: inline-flex; gap: 4px; }
button { font: inherit; color: inherit; background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 6px 12px; cursor: pointer; }
button:hover { border-color: var(--dim); }
button.on { border-color: var(--accent); color: var(--accent); }
button.primary { background: var(--accent); border-color: var(--accent); color: var(--void); font-weight: 600; }
button.swatch { width: 20px; height: 20px; padding: 0; border-radius: 50%; border: 2px solid transparent; }
button.swatch.on { border-color: var(--fg); }
.error { color: var(--danger); }
.reacts { position: absolute; inset: 0; pointer-events: none; overflow: hidden; }
.react { position: absolute; bottom: 8%; font-size: 28px; line-height: 1; animation: arugula-float 2.4s ease-out forwards; }
.head .react { font-size: 16px; }
@keyframes arugula-float { 0% { transform: translateY(0) scale(0.6); opacity: 0; } 12% { transform: translateY(-8%) scale(1.1); opacity: 1; } 75% { opacity: 1; } 100% { transform: translateY(-160%) scale(1); opacity: 0; } }
button.emoji { padding: 4px 7px; font-size: 16px; line-height: 1; background: transparent; }
`;
