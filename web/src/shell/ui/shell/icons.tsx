// The workspace's icons (M108): plain strokes in the text's color, so
// every palette draws them. Each sits beside a label or carries the
// button's aria-label; none is the only sign of anything.

import type { JSX } from "preact";

const Svg = ({ children, size = 18 }: { children: JSX.Element | JSX.Element[]; size?: number }) => (
  <svg
    class="icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {children}
  </svg>
);

/** Arugula's own mark (public/icon.svg): a prompt and a cursor. */
export const Mark = () => (
  <svg class="icon mark" width="22" height="22" viewBox="0 0 64 64" aria-hidden="true">
    <rect width="64" height="64" rx="14" fill="var(--raised)" />
    <path d="M16 22l12 10-12 10" fill="none" stroke="var(--accent)" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M33 44h15" stroke="var(--fg)" stroke-width="5" stroke-linecap="round" />
  </svg>
);

export const SessionsIcon = () => (
  <Svg>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M9 4.5v15M5.5 8.5h1.5M5.5 11.5h1.5" />
  </Svg>
);

export const PanesIcon = ({ size = 18 }: { size?: number }) => (
  <Svg size={size}>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M12 4.5v15M12 12h8.5" />
  </Svg>
);

export const SwarmIcon = () => (
  <Svg>
    <circle cx="6" cy="7" r="2" />
    <circle cx="18" cy="6" r="2" />
    <circle cx="12" cy="13" r="2" />
    <circle cx="6" cy="18.5" r="2" />
    <circle cx="18" cy="18" r="2" />
    <path d="M7.6 8.3l2.8 3.3M16.4 7.3l-2.9 4.2M10.5 14.3l-3 2.9M13.6 14.2l3 2.6" />
  </Svg>
);

export const ChatIcon = () => (
  <Svg>
    <path d="M4.5 6.5a2 2 0 0 1 2-2h11a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H10l-4 3.5v-3.5h0.5a2 2 0 0 1-2-2z" />
  </Svg>
);

export const AgentsIcon = () => (
  <Svg>
    <rect x="5" y="8" width="14" height="11" rx="3" />
    <path d="M12 8V4.5M10 4.5h4" />
    <circle cx="9.5" cy="13.5" r="1" />
    <circle cx="14.5" cy="13.5" r="1" />
  </Svg>
);

export const DocsIcon = () => (
  <Svg>
    <path d="M7 4h7l4 4v12H7z" />
    <path d="M14 4v4h4M10 12h5M10 15.5h5" />
  </Svg>
);

export const LinkIcon = () => (
  <Svg size={14}>
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
  </Svg>
);

export const SettingsIcon = () => (
  <Svg>
    <path d="M4 7h9M17 7h3M4 12h3M11 12h9M4 17h11M19 17h1" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="12" r="2" />
    <circle cx="17" cy="17" r="2" />
  </Svg>
);

export const SearchIcon = ({ size = 15 }: { size?: number }) => (
  <Svg size={size}>
    <circle cx="11" cy="11" r="6" />
    <path d="M20 20l-4.5-4.5" />
  </Svg>
);

export const PlusIcon = ({ size = 15 }: { size?: number }) => (
  <Svg size={size}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

export const MoreIcon = ({ size = 15 }: { size?: number }) => (
  <Svg size={size}>
    <circle cx="6" cy="12" r="0.8" />
    <circle cx="12" cy="12" r="0.8" />
    <circle cx="18" cy="12" r="0.8" />
  </Svg>
);

export const BranchIcon = ({ size = 13 }: { size?: number }) => (
  <Svg size={size}>
    <circle cx="7" cy="6" r="2" />
    <circle cx="7" cy="18" r="2" />
    <circle cx="17" cy="8" r="2" />
    <path d="M7 8v8M17 10c0 4-6 3-9 6.5" />
  </Svg>
);

export const TabsIcon = ({ size = 12 }: { size?: number }) => (
  <Svg size={size}>
    <rect x="4" y="6" width="16" height="13" rx="2" />
    <path d="M4 10h16M9 6v4" />
  </Svg>
);
