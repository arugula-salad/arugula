// The workspace's icons (M108): plain strokes in the text's color, so
// every palette draws them. Each sits beside a label or carries the
// button's aria-label; none is the only sign of anything. The rail's are in
// shared/ui/rail-icons.tsx (#376), for the platform page's rail too.

import { Svg } from "../../../shared/ui/rail-icons";

export { AgentsIcon, BoardsIcon, ChatIcon, DocsIcon, Mark, PanesIcon, ProjectsIcon, SessionsIcon, SettingsIcon, SwarmIcon } from "../../../shared/ui/rail-icons";

export const LinkIcon = () => (
  <Svg size={14}>
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
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

/** nvim's mark (#1734): two uprights and the diagonal between them, as
 * a stroke. The pane header draws it for an nvim editor block, and the
 * swarm's preview beside an nvim's file. */
export const NVIM_GLYPH = "M6 20V5l12 14V4M6 5l-2 2M18 19l2-2";

export const NvimIcon = ({ size = 14 }: { size?: number }) => (
  <Svg size={size}>
    <path d={NVIM_GLYPH} />
  </Svg>
);
