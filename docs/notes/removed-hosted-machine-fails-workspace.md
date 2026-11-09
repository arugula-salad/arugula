---
section: Fixed
title: A removed cloud machine fails its workspace
line: Removing a cloud workspace's machine at Arugula control now fails the workspace, saying so, so its runs end and its sessions stop waiting on it.
headline: no
---
Removing a cloud workspace's machine changed only Arugula control: the workspace still showed ready, and its runs stayed running. Now the workspace fails, saying its machine was removed, and its box is paused until you rebuild or delete it. A rebuild makes a new box and moves the workspace's sessions to it.
