// Inside the desktop app (crates/desktop, M46). The app's init script sets
// `window.__arugulaApp` ({ name, platform }) on every page it opens.
//
// - The client's bar is the window's titlebar: `html[data-desktop]` lets the
//   stylesheet leave room for macOS's window buttons, and on Linux (no
//   decorations) the bar carries its own (ui/window-buttons.tsx). When
//   AppKit shows its native tab bar, the app sets `html[data-native-tabs]`
//   and `--native-tabs` (its height), and the stylesheet moves the bar
//   below it (#323).
// - On macOS the app's menus take only the Mac's own keys (Cmd-Q, H,
//   Option-H, Shift-W) and Edit's, so Cmd-W, T, N and U reach the page: Cmd-W
//   closes the pane (not the window), Cmd-T opens a tab, Cmd-N a window, and
//   Cmd-U attaches files (as Claude's app does): the agent composer's 📎 when
//   one has focus or is the active pane, else the active terminal's
//   *Attach file…* (M70). They're in ui/keys.ts's table.

/** `signedIn`: whether the app is signed in to the machine's control
 * (#1580); absent from apps before it. */
type AppInfo = { name?: string; platform?: "macos" | "linux"; signedIn?: boolean };

export function desktopApp(): AppInfo | null {
  return (globalThis as { __arugulaApp?: AppInfo }).__arugulaApp ?? null;
}

export function desktopPlatform(): "macos" | "linux" | null {
  return desktopApp()?.platform ?? null;
}

/** The Tauri window this page is in, when the app lets the page move it. */
export function tauriWindow(): {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
} | null {
  const t = (globalThis as { __TAURI__?: { window?: { getCurrentWindow?: () => unknown } } }).__TAURI__;
  return (t?.window?.getCurrentWindow?.() as ReturnType<typeof tauriWindow>) ?? null;
}

/** The app's commands, when the page may call them (control's page may
 * call the app's notifications', crates/desktop/src/notices.rs). */
export function tauriInvoke(): (<T>(cmd: string, args?: Record<string, unknown>) => Promise<T>) | null {
  const t = (globalThis as { __TAURI__?: { core?: { invoke?: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T> } } }).__TAURI__;
  return desktopApp() && t?.core?.invoke ? t.core.invoke : null;
}

/** Open `url` in the person's browser, from control's page or the platform
 * page in the app (#364): `window.open` keeps control's own pages in an app
 * window. Returns whether the app took it; false outside the app, for the
 * caller's own `window.open`. An app without the command rejects, and then
 * this opens it the old way. Call it in the click, for the gesture. */
export function openOutside(url: string): boolean {
  const invoke = tauriInvoke();
  if (!invoke) return false;
  invoke("open_outside", { url }).catch(() => {
    window.open(url, "_blank", "noopener");
  });
  return true;
}

/** A new window of the app, showing `pane` (or the same layout). */
export function openInNewWindow(pane?: number) {
  window.open(pane === undefined ? "/" : `/#pane=${pane}`, "_blank");
}

/** Mark the page for the stylesheet. */
export function setupDesktop() {
  const platform = desktopPlatform();
  if (platform) document.documentElement.dataset.desktop = platform;
}
