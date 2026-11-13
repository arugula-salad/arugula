// Settings is one page in two places (Arugula #695): what needs a machine or
// a device's keys is on control's page (`#settings`), what the platform
// holds is on `/settings`, and neither origin can draw the other's. Each
// page's index lists the other's sections from here, as plain links. Claude
// isn't listed: `/settings` shows it only once there's something in it.

export interface SettingsSection {
  id: string;
  title: string;
}

/** The sections of `#settings`, on control's page. */
export const SHELL_SECTIONS: SettingsSection[] = [
  { id: "device", title: "This device" },
  { id: "all-devices", title: "All your devices" },
  { id: "preferences", title: "Preferences" },
  { id: "machine", title: "This machine" },
  { id: "account", title: "Account" },
  { id: "help", title: "Help" },
];

/** The sections of `/settings`, on the platform page. */
export const PLATFORM_SECTIONS: SettingsSection[] = [
  { id: "notifications", title: "Chat notifications" },
  { id: "quiet-hours", title: "Quiet hours" },
  { id: "linked", title: "Linked products" },
  { id: "agents", title: "Agents on your machines" },
  { id: "tokens", title: "Your agent tokens" },
  { id: "chat-alerts", title: "Chat alerts" },
];
