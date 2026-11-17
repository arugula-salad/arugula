# Release notes drafts

Notes for a hand cut (a minor or a major) while they're being written. They sit
here, not in `docs/releases/`, because a `docs/releases/X.Y.Z.md` with no tag yet
goes into What's new at the next build and holds its notes back from the release
train's patches (`scripts/release-notes.mjs`'s `promised`). Nothing reads this
folder.

On the day:

1. `just release minor --dry-run --notes docs/releases/drafts/X.Y.Z.md` names the
   commit and lists the PRs since the last tag. Any marked `<- not in the notes`
   needs an entry or a `no-notes` line. `node scripts/release-notes.mjs LAST_TAG
   COMMIT X.Y.Z` writes those entries from their `docs/notes/`. If a patch went out
   since the draft was written, drop the entries and headlines it already had:
   a headline id can't be in two releases.
2. `just release minor COMMIT --notes docs/releases/drafts/X.Y.Z.md --rare`, then
   the app the same way with `--app` (no `--rare`).
3. The cut commits the notes to `docs/releases/`. Delete the draft.
