// The Labs half of the web client, as one chunk: sandboxes, studio
// apps and the app and workspace blocks, with the code behind
// them. Nothing in the main bundle imports this file statically; the
// page reaches it only through `loadLabs()` (./labs-load), and only when the
// machine it is on has labs. Anything the main bundle needs from these files
// at once (`openWorkspace`, the places bar) lives outside
// them. The swarm's extra views (city, hive, timeline) are chunks of their
// own, fetched when one is picked.

export { AgentsPage } from "./ui/agents-page";
export { DocsPage } from "./ui/docs-page";
export { AppsLayer, pickApp } from "./ui/apps";
export { SandboxesLayer, openSandboxes } from "./ui/sandboxes";
export { agentsBlock } from "./blocks/agents";
export { appBlock } from "./blocks/app";
export { workspaceBlock } from "./blocks/workspace";
