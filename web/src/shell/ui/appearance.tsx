// Appearance… in the account menu: this device's palette, finish and
// corners. Each choice applies at once, so the page behind is the preview.

import { useEffect, useState } from "preact/hooks";
import { appearance, CORNERS, darkDefault, FINISHES, onAppearance, palette, PALETTES, setAppearance, type Palette } from "../../shared/appearance";

let shown = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openAppearance() {
  shown = true;
  changed();
}

export function AppearanceLayer() {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    const off = onAppearance(fn);
    return () => {
      listeners.delete(fn);
      off();
    };
  }, []);
  if (!shown) return null;
  const close = () => {
    shown = false;
    changed();
  };
  return <AppearanceDialog close={close} />;
}

function AppearanceDialog({ close }: { close: () => void }) {
  const a = appearance();
  const dark = PALETTES.find((p) => p.id === darkDefault())!.name;
  // Nothing picked yet: the default's card is the one on.
  const picked = a.palette === "default" ? darkDefault() : a.palette;
  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Appearance" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt appearance" data-appearance onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Appearance</h2>
        <p class="hint">On this device. The terminals follow the palette.</p>
        <button class="appearance-control-look" data-control-look onClick={() => setAppearance({ palette: "slate", finish: "flat", corners: "soft" })}>
          <strong>Use Control look</strong>
          <span>Near-black Slate, cyan focus, soft pane corners</span>
        </button>
        <h3>Palette</h3>
        <div class="appearance-palettes" role="radiogroup" aria-label="Palette">
          <PaletteCard
            p={palette({ ...a, palette: "system" })}
            id="system"
            name="Match system"
            hint={`${dark} or Latte`}
            on={a.palette === "system"}
          />
          {PALETTES.map((p) => (
            <PaletteCard key={p.id} p={p} id={p.id} name={p.name} on={picked === p.id} />
          ))}
        </div>
        <h3>Finish</h3>
        <div class="appearance-finishes" role="radiogroup" aria-label="Finish">
          {FINISHES.map((f) => (
            <button
              key={f.id}
              role="radio"
              aria-checked={a.finish === f.id}
              class={a.finish === f.id ? "appearance-finish on" : "appearance-finish"}
              data-finish-choice={f.id}
              data-look={f.id}
              onClick={() => setAppearance({ finish: f.id })}
            >
              <span class="appearance-finish-sample" aria-hidden="true">
                <span class="sample-bar" />
                <span class="sample-button">OK</span>
              </span>
              <b>{f.name}</b>
              <span class="dim">{f.hint}</span>
            </button>
          ))}
        </div>
        <h3>Corners</h3>
        <div class="appearance-corners" role="radiogroup" aria-label="Corners">
          {CORNERS.map((c) => (
            <button
              key={c.id}
              role="radio"
              aria-checked={a.corners === c.id}
              class={a.corners === c.id ? "on" : undefined}
              data-corners-choice={c.id}
              onClick={() => setAppearance({ corners: c.id })}
            >
              <span class={`corner-sample ${c.id}`} aria-hidden="true" />
              {c.name}
            </button>
          ))}
        </div>
        <div class="prompt-buttons">
          <button
            type="button"
            onClick={() => setAppearance({ palette: "default", finish: "flat", corners: "square" })}
            disabled={a.palette === "default" && a.finish === "flat" && a.corners === "square"}
          >
            Reset
          </button>
          <button type="button" class="primary" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

/** A palette as a tiny window: its bar, its background with text, and its
 * colors in a row. */
function PaletteCard({ p, id, name, hint, on }: { p: Palette; id: string; name: string; hint?: string; on: boolean }) {
  const u = p.ui;
  return (
    <button
      role="radio"
      aria-checked={on}
      class={on ? "appearance-palette on" : "appearance-palette"}
      data-palette-choice={id}
      onClick={() => setAppearance({ palette: id })}
    >
      <span class="palette-sample" style={{ background: u.bg, borderColor: u.raised }} aria-hidden="true">
        <span class="palette-sample-bar" style={{ background: u.surface }}>
          <span style={{ background: u.accent }} />
        </span>
        <span class="palette-sample-text" style={{ color: u.fg }}>
          <span style={{ color: u.ok }}>❯</span> ls <span style={{ color: u.dim }}>~/src</span>
        </span>
        <span class="palette-sample-dots">
          {[u.danger, u.ok, u.warn, u.info, u.accent, u.teal].map((c) => (
            <span key={c} style={{ background: c }} />
          ))}
        </span>
      </span>
      <span class="palette-name">{name}</span>
      {hint ? <span class="palette-hint dim">{hint}</span> : null}
    </button>
  );
}
