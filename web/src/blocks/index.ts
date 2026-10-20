// Every block type's renderer, registered on import.
import "./browser";
import "./agent";
import "./chat";
import "./editor";
import "./remote";
import "./diff";
import "./file";
import "./lazy";
import "./forge";
import "./invite";
import "./doc";
import "./cloud-workspace";

export { makeBlockView, type BlockView } from "./view";
export { openPort } from "./browser";
export { openEditor } from "./editor";
export { newRemote, remoteHosts, remotes } from "./remote";
export { openChanges, openFile } from "./diff";
export { isWorkspace, openAgents, openWorkspace, useWorkspaceDir } from "./open-labs";
export { openIssue, openPr } from "./forge";
