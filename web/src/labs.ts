// The Labs half of the web client, as one chunk: chat, huddles, sandboxes,
// studio apps and the Fountain, app and workspace blocks, with the call code
// behind them. Nothing in the main bundle imports this file statically; the
// page reaches it only through `loadLabs()` (./labs-load), and only when the
// machine it is on has labs. Anything the main bundle needs from these files
// at once (the chat route, `openWorkspace`, the places bar) lives outside
// them. The swarm's extra views (city, hive, timeline) are chunks of their
// own, fetched when one is picked.

export { activeHuddle } from "./call";
export { HuddleBar, HuddleButton, huddleItems } from "./ui/huddle";
export { ChatPage } from "./ui/chat";
export { AppsLayer, pickApp } from "./ui/apps";
export { SandboxesLayer, openSandboxes } from "./ui/sandboxes";
export { agentsBlock } from "./blocks/agents";
export { appBlock } from "./blocks/app";
export { fountainBlock } from "./blocks/fountain";
export { workspaceBlock } from "./blocks/workspace";
