// What control pushes an account about (#468), on every device it turned
// on: the menu's switches (shell/ui/notify.tsx) and the app's Settings ›
// You › Phones and browsers (Arugula #1310) draw these three.
// `GET /api/push/prefs` answers them; `POST` with some sets those.

export interface PushPrefs {
  chat: boolean;
  machines: boolean;
  projects: boolean;
}

/** The kinds, as the menu and Settings say them. */
export const PUSH_KINDS: { key: keyof PushPrefs; label: string; hint: string }[] = [
  { key: "chat", label: "Chat", hint: "Messages in team channels, DMs and pane threads" },
  { key: "machines", label: "Machines", hint: "A pane or an agent on your machines that needs you" },
  { key: "projects", label: "Projects", hint: "Your inbox, deploys, runs and workspaces" },
];
