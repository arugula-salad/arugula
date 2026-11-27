// Appearance: this device's theme, preset and Look's three sliders
// (design lift §4.3, §4.4). Each change applies at once, so the page is the preview. Drawn in
// place in Settings (You's Appearance, the daemon's `#settings`; Arugula
// #1318), and as a dialog, Appearance…, where there's no Settings page to
// go to: the phone's settings menu and the command palette.

import { useEffect, useState } from "preact/hooks";
import { both, useBindings } from "../keys";
import { useSubscribe } from "./hooks";
import { Modal } from "./modal";
import { appearance, isDefaultAppearance, lookSliders, onAppearance, resetAppearance, setAppearance, setPreset, THEMES } from "../appearance";
import { PRESETS, type Sliders } from "../kit/look";

let shown = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openAppearance() {
  shown = true;
  changed();
}

export function AppearanceLayer() {
  // Look (Lift 3, #1479): `s` from anywhere but a field or a terminal; the
  // top bar's Look too. The design reference keeps its own `s` (its Look,
  // that page only).
  const away = () => location.pathname !== "/design";
  useBindings(
    [
      { id: "look", label: "Look", group: "app", keys: both("key:s"), when: away, run: () => (shown ? close() : openAppearance()) },
      // A step of the theme (light ↔ dark) and of depth (flat → deep, round again).
      { id: "look.theme", label: "Light or dark", group: "app", keys: both("key:t"), when: away, run: () => setAppearance({ theme: themeNow() === "light" ? "dark" : "light" }) },
      { id: "look.depth", label: "Depth, a step deeper", group: "app", keys: both("key:d"), when: away, run: () => setAppearance({ depth: nextDepth(appearance().depth) }) },
    ],
    [],
  );
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
  return <AppearanceDialog close={close} />;
}

/** The theme as it shows now: System's is the system's. */
const themeNow = () => {
  const t = appearance().theme;
  return t === "system" ? (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark") : t;
};

/** Depth's next step of a quarter, back to flat past deep. */
const nextDepth = (d: number) => (d >= 1 ? 0 : Math.min(1, Math.round(d * 4 + 1) / 4));

function close() {
  shown = false;
  changed();
}

/** Back to nothing picked: Dark, Arugula and its sliders. */
export function ResetAppearance() {
  useSubscribe(onAppearance);
  return (
    <button class="secondary" type="button" onClick={resetAppearance} disabled={isDefaultAppearance()}>
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
 * Appearance, the daemon's `#settings`): the theme and the preset, then
 * the sliders. */
export function AppearanceControls() {
  useSubscribe(onAppearance);
  const a = appearance();
  const s = lookSliders(a);
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
      <h3 id="appearance-preset">Preset</h3>
      <div class="seg" role="radiogroup" aria-labelledby="appearance-preset">
        {PRESETS.map((p) => (
          <button key={p.id} type="button" role="radio" aria-checked={a.preset === p.id} class={a.preset === p.id ? "on" : undefined} data-preset-choice={p.id} onClick={() => setPreset(p.id)}>
            {p.name}
          </button>
        ))}
      </div>
      <h3>Look</h3>
      <div class="appearance-sliders">
        {LOOK.map((k) => (
          <label key={k.key} class="appearance-slider">
            <span class="dim">{k.ends[0]}</span>
            <input
              type="range"
              min={0}
              max={100}
              aria-label={k.name}
              data-look-slider={k.key}
              value={Math.round(s[k.key] * 100)}
              onInput={(e) => setAppearance({ [k.key]: +(e.target as HTMLInputElement).value / 100 })}
            />
            <span class="dim">{k.ends[1]}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
