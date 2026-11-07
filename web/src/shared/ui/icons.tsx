// Arugula's icons, all of them, in one file (#691): plain strokes in the
// text's color on a 24 grid, so every palette draws them. Each sits beside a
// label or carries the button's aria-label; none is the only sign of
// anything. `Svg` is the one wrapper: it sets the stroke (1.7, or 2 at `xs`,
// so a 12px icon still draws about 1px) and the size, which is a name
// (`xs` 12, `sm` 14, `md` 16, `lg` 18: `--icon-*` in style.css) or, until
// the callers move to the names, a number.
//
// The chevrons, check, close, expand, external link, command, copy, pencil,
// star, reload, back arrow, trash and arrows are Lucide's paths
// (https://lucide.dev, ISC licence; some derive from Feather, MIT; both
// notices are in THIRD_PARTY.md), copied in and drawn at this file's stroke.

import type { ComponentChildren } from "preact";

export type IconSize = "xs" | "sm" | "md" | "lg";
const PX: Record<IconSize, number> = { xs: 12, sm: 14, md: 16, lg: 18 };

export const Svg = ({ children, size = 18, class: extra }: { children?: ComponentChildren; size?: IconSize | number; class?: string }) => {
  const px = typeof size === "number" ? size : PX[size];
  const named = typeof size === "number" ? "" : ` icon-${size}`;
  return (
    <svg
      class={`icon${named}${extra ? ` ${extra}` : ""}`}
      width={px}
      height={px}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={size === "xs" ? 2 : 1.7}
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
};

type Sized = { size?: IconSize | number };

/** Arugula's own mark (public/icon.svg): a prompt and a cursor. */
export const Mark = () => (
  <svg class="icon mark" width="22" height="22" viewBox="0 0 64 64" aria-hidden="true">
    <rect width="64" height="64" rx="14" fill="var(--raised)" />
    <path d="M16 22l12 10-12 10" fill="none" stroke="var(--accent)" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M33 44h15" stroke="var(--fg)" stroke-width="5" stroke-linecap="round" />
  </svg>
);

// The rail's (M108, #376; the platform page's rail too).

export const SessionsIcon = () => (
  <Svg>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M9 4.5v15M5.5 8.5h1.5M5.5 11.5h1.5" />
  </Svg>
);

export const PanesIcon = ({ size = 18 }: Sized) => (
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

export const BoardsIcon = () => (
  <Svg>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M8 8v3M12 8v6M16 8v4" />
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

export const SettingsIcon = () => (
  <Svg>
    <path d="M4 7h9M17 7h3M4 12h3M11 12h9M4 17h11M19 17h1" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="12" r="2" />
    <circle cx="17" cy="17" r="2" />
  </Svg>
);

/** Projects: a grid of four. */
export const ProjectsIcon = () => (
  <Svg>
    <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
    <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
    <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
    <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
  </Svg>
);

export const PlusIcon = ({ size = 15 }: Sized) => (
  <Svg size={size}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

// The workspace's (M108).

export const LinkIcon = ({ size = 14 }: Sized) => (
  <Svg size={size}>
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
  </Svg>
);

/** One magnifier for the workspace and chat: the workspace's drew a little
 * smaller than chat's, so this sits between them (no more than a pixel off
 * either at the sizes they use). */
export const SearchIcon = ({ size = 15 }: Sized) => (
  <Svg size={size}>
    <circle cx="10.5" cy="10.5" r="6.5" />
    <path d="M15.1 15.1 20.5 20.5" />
  </Svg>
);

/** Three dots, which draw as a fill and ignore the stroke. */
export const MoreIcon = ({ size = 15 }: Sized) => (
  <Svg size={size}>
    <circle cx="6" cy="12" r="0.8" />
    <circle cx="12" cy="12" r="0.8" />
    <circle cx="18" cy="12" r="0.8" />
  </Svg>
);

export const BranchIcon = ({ size = 13 }: Sized) => (
  <Svg size={size}>
    <circle cx="7" cy="6" r="2" />
    <circle cx="7" cy="18" r="2" />
    <circle cx="17" cy="8" r="2" />
    <path d="M7 8v8M17 10c0 4-6 3-9 6.5" />
  </Svg>
);

export const TabsIcon = ({ size = 12 }: Sized) => (
  <Svg size={size}>
    <rect x="4" y="6" width="16" height="13" rx="2" />
    <path d="M4 10h16M9 6v4" />
  </Svg>
);

/** nvim's mark (#1734): two uprights and the diagonal between them, as
 * a stroke. The pane header draws it for an nvim editor block, and the
 * swarm's preview beside an nvim's file. */
export const NVIM_GLYPH = "M6 20V5l12 14V4M6 5l-2 2M18 19l2-2";

export const NvimIcon = ({ size = 14 }: Sized) => (
  <Svg size={size}>
    <path d={NVIM_GLYPH} />
  </Svg>
);

/** Two corners out (focus), or in (restore). */
export const FocusIcon = ({ on, size }: { on: boolean } & Sized) => (
  <Svg size={size ?? 12}>{on ? <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" /> : <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />}</Svg>
);

export const ShareIcon = ({ size = 14 }: Sized) => (
  <Svg size={size}>
    <path d="M4 12v7a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7" />
    <path d="M16 6l-4-4-4 4" />
    <path d="M12 2v13" />
  </Svg>
);

/** The live switch's camera. */
export const CameraIcon = ({ size = 18 }: Sized) => (
  <Svg size={size}>
    <rect x="3" y="6.5" width="12.5" height="11" rx="2" />
    <path d="M15.5 10.5l5-3v9l-5-3z" />
  </Svg>
);

// A PR's and an issue's block (item-block).

export const PrIcon = ({ size = 16 }: Sized) => (
  <Svg size={size}>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="18" r="2.5" />
    <path d="M6 8.5v7M18 15.5V10a3 3 0 0 0-3-3h-3m2-2.5L12 7l2 2.5" />
  </Svg>
);

export const IssueIcon = ({ size = 16 }: Sized) => (
  <Svg size={size}>
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="1.5" fill="currentColor" />
  </Svg>
);

export const MergeIcon = ({ size = 13 }: Sized) => (
  <Svg size={size}>
    <circle cx="6" cy="5" r="2.5" />
    <circle cx="6" cy="19" r="2.5" />
    <circle cx="18" cy="14" r="2.5" />
    <path d="M6 7.5v9M18 11.5C18 8 12 9 7.5 6.5" />
  </Svg>
);

const FORGE_PATH = {
  github: "M9 19c-5 1.5-5-2.5-7-3m14 6v-3.9a3.4 3.4 0 0 0-.9-2.6c3.100-.4 6.400-1.500 6.400-7A5.400 5.400 0 0 0 20 4.800 5.100 5.100 0 0 0 19.900 1S18.700.7 16 2.500a13.400 13.400 0 0 0-7 0C6.300.7 5.100 1 5.100 1A5.100 5.100 0 0 0 5 4.800a5.400 5.400 0 0 0-1.500 3.800c0 5.400 3.300 6.600 6.400 7A3.400 3.400 0 0 0 9 18.100V22",
  gitlab: "M12 21 3 14l2.500-8 2.500 6h8l2.500-6L21 14z",
  forgejo: "M7 4v6a3 3 0 0 0 3 3h4a3 3 0 0 0 3-3V4M12 13v7",
  arugula: "M12 3c5 0 9 4 9 9-4 0-9-1-9-9zM12 3C7 3 3 7 3 12c4 0 9-1 9-9zM12 21v-9",
};

/** Where an item lives: its forge's mark, or Arugula's own. */
export const SourceGlyph = ({ kind, size = 16 }: { kind: keyof typeof FORGE_PATH } & Sized) => (
  <Svg size={size}>
    <path d={FORGE_PATH[kind]} />
  </Svg>
);

// The bell: the header's Needs you, and a chat room's notifications. One
// outline in the text's colour, so it reads the same everywhere.

const Bell = ({ size, children }: { size: number; children?: ComponentChildren }) => (
  <Svg size={size}>
    <path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z" />
    <path d="M10 20.5a2 2 0 0 0 4 0" />
    {children}
  </Svg>
);

export const BellIcon = ({ size = 18 }: { size?: number }) => <Bell size={size} />;

/** The bell, struck through: notifications off. */
export const BellOffIcon = ({ size = 18 }: { size?: number }) => (
  <Bell size={size}>
    <path d="M4 4l16 16" />
  </Bell>
);

// Chat's (UI audit #169, #501, #757), where a text glyph reads as something
// else or as nothing at 13px. They were drawn on a 16 grid at 1.4; these are
// the same shapes at 1.5 times the coordinates on the 24 grid, at 1.7.

export function BookmarkIcon({ on, size = 14 }: { on?: boolean } & Sized) {
  return (
    <Svg size={size}>
      <path d="M6 3.75h12v16.5l-6-4.5-6 4.5z" fill={on ? "currentColor" : "none"} />
    </Svg>
  );
}

/** Add a reaction: a smiley, on the hover bar and the reaction row (#501; ☺
 * read as `@` at text size). */
export function ReactIcon({ size = 14 }: Sized) {
  return (
    <Svg size={size}>
      <circle cx="12" cy="12" r="9.3" />
      <path d="M8.1 14.4c.9 1.65 2.25 2.4 3.9 2.4s3-.75 3.9-2.4" />
      <path d="M8.85 9.3v.9M15.15 9.3v.9" stroke-width="2.4" />
    </Svg>
  );
}

/** A machine's agent (#428): beside the machine in the `@` picker and in a
 * message that mentions it, and on the room's agents chip. */
export function MachineIcon({ size = 15 }: Sized) {
  return (
    <Svg size={size} class="machine-icon">
      <rect x="4.5" y="8.25" width="15" height="10.5" rx="3" />
      <path d="M12 8.25V4.5M9.75 4.5h4.5" />
      <circle cx="9" cy="13.5" r="0.9" fill="currentColor" />
      <circle cx="15" cy="13.5" r="0.9" fill="currentColor" />
    </Svg>
  );
}

/** What the message menu's rows carry (#757): sized by the menu's lead
 * column, in place of the emoji they were. */
const Row = ({ children }: { children: ComponentChildren }) => <Svg size={16}>{children}</Svg>;

export const QuoteIcon = () => (
  <Row>
    <path d="M4.5 6h7.5v6H8.25C8.25 15 6.75 16.5 4.5 17.25M13.5 6h7.5v6h-3.75c0 3-1.5 4.5-3.75 5.25" />
  </Row>
);

export const UnreadIcon = () => (
  <Row>
    <circle cx="12" cy="12" r="8.25" />
    <path d="M12 3.75v16.5" />
  </Row>
);

export const ClockIcon = () => (
  <Row>
    <circle cx="12" cy="12" r="8.25" />
    <path d="M12 7.5v4.8l3 1.95" />
  </Row>
);

export const PinIcon = () => (
  <Row>
    <path d="M9 3.75h6l-.75 6 3 3.75H6.75l3-3.75zM12 13.5v6.75" />
  </Row>
);

export const ThreadIcon = () => (
  <Row>
    <path d="M9.75 5.25 4.5 10.5l5.25 5.25M4.5 10.5h9a6 6 0 0 1 6 6v2.25" />
  </Row>
);

// The rest are Lucide's. Sizes are for the steps that use them: a caret or a
// fold beside text is `xs`, a bar button's icon `sm`, a menu's lead `md`.

/** A caret or a fold's arrow. `open` is for a fold: down while open, right
 * while closed. */
export const ChevronIcon = ({ dir = "down", open, size = "xs" }: { dir?: "down" | "right" | "up"; open?: boolean } & Sized) => {
  const d = open === undefined ? dir : open ? "down" : "right";
  return (
    <Svg size={size}>
      <path d={d === "down" ? "m6 9 6 6 6-6" : d === "right" ? "m9 18 6-6-6-6" : "m18 15-6-6-6 6"} />
    </Svg>
  );
};

export const CheckIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M20 6 9 17l-5-5" />
  </Svg>
);

export const CloseIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M18 6 6 18" />
    <path d="m6 6 12 12" />
  </Svg>
);

/** Two arrows out along a diagonal (⤢). */
export const ExpandIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M15 3h6v6" />
    <path d="m21 3-7 7" />
    <path d="m3 21 7-7" />
    <path d="M9 21H3v-6" />
  </Svg>
);

/** Opens elsewhere (↗). */
export const ExternalIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M15 3h6v6" />
    <path d="M10 14 21 3" />
    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
  </Svg>
);

export const CommandIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3" />
  </Svg>
);

export const CopyIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
    <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
  </Svg>
);

/** A pencil. The message menu's Edit is this one. */
export const EditIcon = ({ size = 16 }: Sized) => (
  <Svg size={size}>
    <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" />
    <path d="m15 5 4 4" />
  </Svg>
);

export const StarIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
  </Svg>
);

export const ReloadIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5" />
  </Svg>
);

export const BackIcon = ({ size = "sm" }: Sized) => (
  <Svg size={size}>
    <path d="m12 19-7-7 7-7" />
    <path d="M19 12H5" />
  </Svg>
);

export const TrashIcon = ({ size = 16 }: Sized) => (
  <Svg size={size}>
    <path d="M10 11v6" />
    <path d="M14 11v6" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
    <path d="M3 6h18" />
    <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
  </Svg>
);

export const ArrowIcon = ({ dir = "right", size = "sm" }: { dir?: "up" | "down" | "right" } & Sized) => (
  <Svg size={size}>
    {dir === "up" ? (
      <path d="m5 12 7-7 7 7M12 19V5" />
    ) : dir === "down" ? (
      <path d="M12 5v14m7-7-7 7-7-7" />
    ) : (
      <path d="M5 12h14m-7-7 7 7-7 7" />
    )}
  </Svg>
);
