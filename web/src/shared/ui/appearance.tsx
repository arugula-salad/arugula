// Appearance: this device's theme and Look's three sliders (design lift
// §4.3). Each change applies at once, so the page is the preview. Drawn in
// place in Settings (You's Appearance, the daemon's `#settings`; Arugula
// #1318), and as a dialog, Appearance…, where there's no Settings page to
// go to: the phone's settings menu and the command palette.

import { useEffect, useState } from "preact/hooks";
import { useSubscribe } from "./hooks";
import { Modal } from "./modal";
import { appearance, onAppearance, setAppearance, THEMES } from "../appearance";
import { SLIDERS, type Sliders } from "../kit/look";

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

const RESET = { theme: "dark", ...SLIDERS } as const;

/** Back to nothing picked: Dark, each slider in the middle. */
export function ResetAppearance() {
  useSubscribe(onAppearance);
  const a = appearance();
  return (
    <button
      class="secondary"
      type="button"
      onClick={() => setAppearance(RESET)}
      disabled={a.theme === RESET.theme && a.light === RESET.light && a.warmth === RESET.warmth && a.depth === RESET.depth}
    >
      Reset
    </button>
  );
}

function AppearanceDialog({ close }: { close: () => void }) {
  return (
    <Modal
      class="appearance"
      title="Appearance"
      close={close}
      root={{ "data-appearance": true }}
      buttons={
        <>
          <ResetAppearance />
          <button class="secondary" type="button" onClick={close}>
            Done
          </button>
        </>
      }
    >
      <AppearanceControls />
    </Modal>
  );
}

/** The sliders, each with the words at its two ends. */
const LOOK: { key: keyof Sliders; name: string; ends: [string, string] }[] = [
  { key: "light", name: "Lightness", ends: ["Lighter", "Darker"] },
  { key: "warmth", name: "Warmth", ends: ["Cool", "Warm"] },
  { key: "depth", name: "Depth", ends: ["Flat", "Deep"] },
];

/** What the dialog holds, drawn in place where Settings is a page (You's
 * Appearance, the daemon's `#settings`): the theme, then the sliders. */
export function AppearanceControls() {
  useSubscribe(onAppearance);
  const a = appearance();
  return (
    <div class="appearance-controls" data-appearance-controls>
      <p class="dim">On this device. The terminals follow the theme.</p>
      <h3 id="appearance-theme">Theme</h3>
      <div class="seg" role="radiogroup" aria-labelledby="appearance-theme">
        {THEMES.map((t) => (
          <button key={t.id} type="button" role="radio" aria-checked={a.theme === t.id} class={a.theme === t.id ? "on" : undefined} data-theme-choice={t.id} onClick={() => setAppearance({ theme: t.id })}>
            {t.name}
          </button>
        ))}
      </div>
      <h3>Look</h3>
      <div class="appearance-sliders">
        {LOOK.map((s) => (
          <label key={s.key} class="appearance-slider">
            <span class="dim">{s.ends[0]}</span>
            <input
              type="range"
              min={0}
              max={100}
              aria-label={s.name}
              data-look-slider={s.key}
              value={Math.round(a[s.key] * 100)}
              onInput={(e) => setAppearance({ [s.key]: +(e.target as HTMLInputElement).value / 100 })}
            />
            <span class="dim">{s.ends[1]}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
