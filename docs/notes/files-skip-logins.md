---
section: Changed
title: Files leaves logins and keys alone
line: Files, file blocks and agents' read_file don't open ~/.ssh, cloud and gh logins or Claude's credentials file; a read-only MCP token's read_file reads in a pane's directory.
headline: no
---
The list is `CREDENTIALS` in `crates/mux/src/fs.rs`. A terminal still reaches them as before.
