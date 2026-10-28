// The hints (./index, DECISIONS.md "Hints come from facts, once per
// account"), in the order they're offered when two facts hold at once. A
// new one is an entry here: its id (never reused), the fact, one line of
// copy, its action and its place.

import type { Hint } from "./index";
import { keyHint } from "../ui/keys";
import { pathOffered } from "./attach";
import { cdTyped } from "./cd";
import { conversationHere } from "./conversations";
import { docOpen } from "./docs";
import { printedLink } from "./links";
import { secondPerson } from "./people";
import { listeningPort } from "./ports";
import { pushWanted } from "./push";
import { restoredPane } from "./restart";
import { smallTeam } from "./teams";

export const HINTS: Hint[] = [
  {
    // A dev server started in a pane: it can be beside the terminal.
    id: "port-listening",
    place: "chip",
    fact: listeningPort,
    copy: (f) => `:${f.args.port} is listening`,
    action: { label: "Open beside", command: "port.open-beside" },
  },
  {
    // A PR or an issue link printed in a pane (`gh pr create`'s).
    id: "pr-link",
    place: "chip",
    fact: printedLink,
    copy: () => "Click opens it here. Shift-click opens your browser",
    action: { label: "Open it", command: "forge.open-link" },
  },
  {
    // A file's path pasted in, or files dragged over the pane.
    id: "attach-file",
    place: "chip",
    fact: pathOffered,
    copy: () => "Attach file puts it on this pane's machine",
    action: { label: "Attach file…", command: "pane.attach" },
  },
  {
    // `cd` typed in a fresh pane.
    id: "cd-picker",
    place: "chip",
    fact: cdTyped,
    copy: () => `${keyHint("picker") ?? "Go to directory"} goes to a directory`,
    action: { label: "Go to directory", command: "pane.picker" },
  },
  {
    // Panes back after a reboot or a daemon restart.
    id: "after-restart",
    place: "chip",
    fact: restoredPane,
    copy: () => "Re-run, or claude --continue, when this machine restarts",
    action: { label: "After a restart…", command: "pane.restart-settings" },
  },
  {
    // The first Doc block.
    id: "docs-page",
    place: "chip",
    fact: docOpen,
    copy: () => "Kept on the Docs page. Ask any agent for a Doc",
    action: { label: "Go to Docs", route: "#docs" },
  },
  {
    // Someone else in the session.
    id: "second-person",
    place: "toast",
    fact: secondPerson,
    copy: () => "Click their avatar to follow. Share › Pair mode to type together",
    action: { label: "Follow", command: "people.follow" },
  },
  {
    // Something wanted you, and this device could have said so.
    id: "push-device",
    place: "toast",
    fact: pushWanted,
    copy: () => "Get these on this device, or your phone",
    action: { label: "Notify this device", command: "push.enable" },
  },
  {
    // A team made, or its second member in.
    id: "team-invite",
    place: "toast",
    fact: smallTeam,
    copy: () => "Invite with a link · Turn on chat for this team",
    action: { label: "Teams…", command: "control.teams" },
  },
  {
    // Claude Code conversations on this machine, none open.
    id: "claude-conversations",
    place: "toast",
    fact: conversationHere,
    copy: () => "Pick up a Claude Code conversation here",
    action: { label: "Claude Code conversations…", command: "conversations.pick" },
  },
];
