// The bell: the header's Needs you, and a chat room's notifications. One
// outline in the text's colour, so it reads the same everywhere.

const Bell = ({ size, children }: { size: number; children?: preact.ComponentChildren }) => (
  <svg class="icon" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z" />
    <path d="M10 20.5a2 2 0 0 0 4 0" />
    {children}
  </svg>
);

export const BellIcon = ({ size = 18 }: { size?: number }) => <Bell size={size} />;

/** The bell, struck through: notifications off. */
export const BellOffIcon = ({ size = 18 }: { size?: number }) => (
  <Bell size={size}>
    <path d="M4 4l16 16" />
  </Bell>
);
