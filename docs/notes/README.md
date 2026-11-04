# Release notes, one file per change

A pull request that people would notice adds a file here, and a release's notes
are built from them (`scripts/release-notes.mjs`). Nobody writes a release's notes
at the end, and a release with no notes in it doesn't go out. The files stay here
after a release: which commit added a file says which release it belongs to.

The file's name is the feature's id for good: lowercase words and dashes, such as
`docs/notes/settings-one-page.md`.

```markdown
---
section: Changed
title: Settings is a page
line: The rail's Settings opens a page on both Panes and Projects.
try: {"route": "/settings"}
needs: control
---
The longer version for the release's page, if the line isn't enough. Markdown.
```

- `section`: Added, Changed, Fixed or Removed.
- `title`: a few words. `line`: one sentence on what it means for someone using
  Arugula. Both show in What's new.
- `try`, `needs` (optional): what *Try it* opens and who may use it, as
  `scripts/whats-new.mjs` describes.
- `headline: no` (optional): only in the release's notes, not in What's new. Use it
  for small fixes, so What's new stays about what's worth stopping for. Its first
  five notes that don't say this are the ones that show.

**A change nobody would notice** says so instead, with a line in a commit message
that says why:

```text
Release-note: none, the tests' fake forge only
```

The `notes` check on a pull request (`scripts/pr-notes-check`) fails until it has one
or the other. A change only to docs, tests, CI or scripts needs neither.
