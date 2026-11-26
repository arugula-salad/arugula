// The top bar (Lift 3, #1479; v6's): where you are, as a crumb whose first
// step is the project (it opens Go to…'s projects), the place's own views,
// then Go to…, Look and, where the page has it, Share. In the desktop
// app on Linux it's the titlebar (M157). Both pages draw it.

import type { ComponentChildren } from "preact";
import { ariaKeys, keyHint } from "../../keys";
import { SearchIcon } from "../../ui/icons";
import { openAppearance } from "../../ui/appearance";
import { WindowButtons } from "../../ui/window-buttons";
import { Crumb, type Step } from "../crumb";
import { Kbd } from "../kbd";
import { openGoTo } from "./goto";
import "../kit.css";

/** `views`: the place's own, after the crumb; `tools`: a machine's (its
 * update, bell and people), after Go to…, so the empty middle between the
 * place's views and Go to… stays the bar's to drag the window by
 * (desktop-xvfb's titlebar claim drags there). */
export function TopBar({ steps, views, tools, share, ...data }: { steps: Step[]; views?: ComponentChildren; tools?: ComponentChildren; share?: ComponentChildren; [data: `data-${string}`]: unknown }) {
  const go = keyHint("goto");
  const look = keyHint("look");
  return (
    <header class="k-top" data-top data-tauri-drag-region {...data}>
      {/* Its gaps move the window in the desktop app, as the bar's do. */}
      <Crumb steps={steps} data-app-path data-tauri-drag-region />
      {views}
      <span class="k-top-fill" data-tauri-drag-region />
      <button
        type="button"
        class="k-top-act"
        data-open-palette
        title={`Go to a place, a pane or a command${go ? ` (${go})` : ""}`}
        aria-keyshortcuts={ariaKeys(go)}
        onClick={() => openGoTo()}
      >
        <SearchIcon size="sm" />
        Go to… {go && <Kbd keys={go} />}
      </button>
      <button type="button" class="k-top-act" data-open-look title={`This device's theme and Look${look ? ` (${look})` : ""}`} aria-keyshortcuts={ariaKeys(look)} onClick={openAppearance}>
        Look {look && <Kbd keys={look} />}
      </button>
      {tools}
      {share}
      <WindowButtons />
    </header>
  );
}
