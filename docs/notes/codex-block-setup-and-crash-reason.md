---
section: Fixed
title: A Codex block starts, or says why it didn't
line: Setting up Codex installs the adapter with its own Codex, a Codex block no longer dies on a `codex` that isn't there, and a crashed adapter's note is its error.
headline: no
---
`arugula setup codex` no longer leaves out the Codex binary the adapter
runs when your machine has none, and a Codex block uses the `codex` on your
`PATH` or in `~/.local/bin` only if there is one. Before, either gap ended
in "The agent exited with code 1: throw er; // Unhandled 'error' event".
When an adapter does crash, the block's note is the error itself, such as
"Error: spawn codex ENOENT", not Node's `throw` line.
