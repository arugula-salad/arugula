// Settings is one page in two places (Arugula #695): what needs a machine or
// a device's keys is on control's page (`#settings`), what the platform
// holds is on `/settings`, and neither origin can draw the other's.
// Control's index lists `/settings`'s sections from here, as plain links;
// `/settings` opens control's page at each thing a device signs, in place
// (Arugula #1108), not from an index. Claude isn't listed: `/settings`
// shows it only once there's something in it.

export interface SettingsSection {
  id: string;
  title: string;
}

/** The sections of `/settings`, on the platform page. */
export const PLATFORM_SECTIONS: SettingsSection[] = [
  { id: "notifications", title: "Chat notifications" },
  { id: "quiet-hours", title: "Quiet hours" },
  { id: "linked", title: "Linked products" },
  { id: "agents", title: "Agents on your machines" },
  { id: "tokens", title: "Your agent tokens" },
  { id: "chat-alerts", title: "Chat alerts" },
];
