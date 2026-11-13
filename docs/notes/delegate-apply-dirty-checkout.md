---
section: Fixed
title: Applying a delegated patch on a dirty checkout
line: Applying a delegated task's patch to a checkout with uncommitted changes no longer says it applied "only in part" when every file landed.
headline: no
---
`delegate {kind: apply}` on a checkout whose files had uncommitted changes
said the patch applied "only in part" even when every file landed. It now
says it applied, and adds that it did so without a three-way merge because
those files (named) had uncommitted changes. "Only in part" stays for a
patch that really left hunks in `.rej` files or conflicts to resolve.
