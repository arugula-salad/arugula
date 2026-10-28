// The hints (./index, DECISIONS.md "Hints come from facts, once per
// account"), in the order they're offered when two facts hold at once. A
// new one is an entry here: its id (never reused), the fact, one line of
// copy, its action and its place.

import type { Hint } from "./index";
import { keyHint } from "../ui/keys";
import { sharedAgents } from "./agents-notify";
import { pathOffered } from "./attach";
import { cdTyped } from "./cd";
import { repoMoved } from "./changes";
import { conversationHere } from "./conversations";
import { docOpen } from "./docs";
import { editorStarted } from "./editor";
import { otherHost } from "./hosts";
import { printedLink } from "./links";
import { commandMarked } from "./marks";
import { passkeyWanted } from "./passkeys";
import { secondPerson } from "./people";
import { claudeEditsBesideNvim, NVIM_HELP, nvimNotTheEditor, nvimStarted, nvimWithoutPlugin, tipCopy, tipTaken } from "./nvim-facts";
import { listeningPort } from "./ports";
import { secretPane } from "./private";
import { pushWanted } from "./push";
import { pushOnControl } from "./push-kinds";
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
    // The Share dialog warned that a pane may show secrets.
    id: "private-pane",
    place: "chip",
    fact: secretPane,
    copy: () => "Make this pane private: nobody you share with sees it",
    action: { label: "Private…", command: "pane.private" },
  },
  {
    // A commit or a checkout in a pane.
    id: "repo-changes",
    place: "chip",
    fact: repoMoved,
    copy: () => "See what changed beside it",
    action: { label: "Changes", command: "changes.open" },
  },
  // nvim (#1735, ./nvim-facts): its help card the first time it runs in a pane,
  // then its chips, seen once answered.
  NVIM_HELP,
  {
    // nvim without arugula.nvim, which Arugula can add (#1733).
    id: "nvim-connect",
    place: "chip",
    fact: nvimWithoutPlugin,
    copy: () => "Arugula can follow this nvim and open files in it",
    action: { label: "Add arugula.nvim", command: "nvim.install-plugin" },
    seenOn: "answer",
    views: { shown: "nvim-connect-shown", acted: "nvim-connect-acted", closed: "nvim-connect-dismissed" },
  },
  {
    // nvim runs, and Open in editor opens something else (#1731).
    id: "nvim-open-here",
    place: "chip",
    fact: nvimNotTheEditor,
    copy: () => "Open files from Arugula in this nvim?",
    action: { label: "Use nvim", command: "editor.use-nvim" },
    seenOn: "answer",
    views: { shown: "nvim-open-here-shown", acted: "nvim-open-here-acted", closed: "nvim-open-here-dismissed" },
  },
  {
    // Claude Code proposes an edit while nvim is connected (#1732).
    id: "nvim-diff",
    place: "chip",
    fact: claudeEditsBesideNvim,
    copy: () => "Review Claude's edits in nvim",
    action: { label: "Show me", command: "nvim.help-card" },
    seenOn: "answer",
    views: { shown: "nvim-diff-shown", acted: "nvim-diff-acted", closed: "nvim-diff-dismissed" },
  },
  {
    // `code` typed in a pane, where blocks can show.
    id: "editor-beside",
    place: "chip",
    fact: editorStarted,
    copy: () => "Open the editor beside it",
    action: { label: "Open in editor", command: "editor.open" },
  },
  {
    // Another machine in the host menu.
    id: "remote-split",
    place: "chip",
    fact: otherHost,
    copy: () => "Split a shell on that machine into this tab",
    action: { label: (f) => `Split right on ${f.args.host}`, command: "remote.split" },
  },
  {
    // A new nvim process in a pane: one key tip, every start, after the
    // one-time hints.
    id: "nvim-key-tip",
    place: "chip",
    fact: nvimStarted,
    copy: tipCopy,
    action: { label: (f) => `:help ${f.args.help}`, command: "nvim.help" },
    seenOn: "answer",
    repeat: "per-fact",
    said: tipTaken,
    views: { shown: "nvim-key-tip-shown", acted: "nvim-key-tip-acted", closed: "nvim-key-tip-dismissed" },
  },
  {
    // A command's first mark (shell integration). Last of the chips: any
    // command makes one.
    id: "command-mark",
    place: "chip",
    fact: commandMarked,
    copy: () => "Right-click the mark: copy its output, run it again",
    action: { label: "Show its menu", command: "mark.menu" },
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
  {
    // This device's push on, through control.
    id: "push-kinds",
    place: "toast",
    fact: pushOnControl,
    copy: () => "Choose what to be told about: chat, machines, projects",
    action: { label: "Notifications…", command: "push.kinds" },
  },
  {
    // A shared session with agents in it, for a guest or a teammate.
    id: "agents-notify",
    place: "toast",
    fact: sharedAgents,
    copy: () => "Get told when its agents need someone",
    action: { label: "Notify me about its agents", command: "agents.notify" },
  },
  {
    // A GitHub-only account, on a device that can make a passkey.
    id: "add-passkey",
    place: "toast",
    fact: passkeyWanted,
    copy: () => "Add a passkey: sign in without GitHub",
    action: { label: "Passkeys…", command: "control.passkeys" },
  },
];
