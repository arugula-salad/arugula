---
title: "A GitHub block says \"no gh here\" or is read-only"
description: "Forge blocks use the gh login on the machine the block runs on."
area: "Forges"
---

# A GitHub block says "no gh here" or is read-only

## You see

A PR or issue block shows one of:

```
no gh here: install GitHub's CLI and `gh auth login`, then refresh
```

```
gh has no login for github.com: `gh auth login --hostname github.com`
```

or it opens with a banner:

```
read-only: no gh login here, so it reads through Arugula control's GitHub App; writes go out as you, so they need your own login (`gh auth login`, then refresh)
```

## Why

The daemon reads and writes GitHub as you, with the token of GitHub's CLI
on the machine the block runs on (`gh auth token`, run in your login
environment; `crates/arugulad-forge/src/github.rs`, `GhToken`). That's
the daemon's machine, which may not be the one you're looking from. With
no `gh` login for github.com, a machine joined to control reads the repo
through control's GitHub App, if it's installed on the repo, but it won't
write as you through it. The token is kept in memory, so a login you add
later is only used after a refresh.

## Do

On the machine the block runs on: install `gh`, run
`gh auth login` (`--hostname` for GitHub Enterprise), then refresh the
block.
