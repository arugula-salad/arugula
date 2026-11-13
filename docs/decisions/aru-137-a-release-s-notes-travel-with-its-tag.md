---
schema: 1
id: "aru-137"
title: "A release's notes travel with its tag"
state: "decided"
area: "Build and release"
source:
  via: "cli"
  harness: "claude-code"
question: "`just release` refused a commit whose tree lacked its notes, so writing them made a new, untested commit at main's moving tip, and later merges cancelled its CI run and needed more notes. How does a release get notes written for one green commit without a commit before its tag?"
options:
  - id: "a"
    label: "Notes in the annotated tag's message; build jobs write them into their checkout"
    how: "`scripts/release bump` puts the notes in the tag's message. `scripts/release stamp`, which already writes the version into a build's checkout, writes them to docs/releases/, and `scripts/release draft` reads them from the tag. A commit after the tag adds them to main beside the client fixtures."
    tradeoff: "Main has a release's notes only after its tag, so a build has to fetch the tag to read them, and an older pick has to fetch the last release's too."
  - id: "b"
    label: "Keep notes in the tree"
    how: "A release's notes are merged to main before its tag, as they were."
    tradeoff: "A commit before every tag, at main's tip, which is the problem."
  - id: "c"
    label: "Take notes from main at build time"
    how: "Build jobs read docs/releases/X.Y.Z.md from main, not the tag's commit."
    tradeoff: "A build no longer follows from its tag: notes written or changed later reach it, and they can describe commits the tag doesn't have."
  - id: "d"
    label: "Have chant publish from a stamped tree"
    how: "Change `chant release publish`, which refuses a tree with anything changed or untracked."
    tradeoff: "A chant change for what stamping does without one; the prototype showed none is needed."
choice:
  option: "a"
  reason: "A prototype on a stand-in forge and real chant 0.124.67 showed stamped notes reach What's New, the cut's green check only reads the commit, and the publish job, which never stamps, sees a clean tree, so the pick is fixed at the start and merges landing meanwhile change nothing (Jake, 2026-11-04)."
rejected:
  - option: "b"
    why: "It's what took hours on 2026-11-04 (0.66.17 became 0.67.0)."
  - option: "c"
    why: "0.66.16's notes listed four commits its tag doesn't contain; a release's notes are written for one commit."
  - option: "d"
    why: "Not needed unless stamping fails, and that's a change in another product."
supersedes: []
evidence:
  - title: "Arugula native #707"
    url: "https://app.arugula.io/issues/iss_8595038215172502e8d8ef6787d79a08"
decided_by: "jhgaylor"
decided_on: "2026-11-04"
reviews: []
constrains:
  - "path:scripts/release"
  - "path:scripts/whats-new.mjs"
  - "path:scripts/tests/release-bump.sh"
  - "path:.github/workflows/release.yml"
  - "path:docs/development.md"
---

# A release's notes travel with its tag

A release is a green commit as it is (`#1986`), so nothing can add its notes to
that commit. Decided (Jake, 2026-11-04):

- **The tag's message holds them.** Between `<!-- release notes -->` and
  `<!-- end of release notes -->`, after the title and before any trailer, so a
  tag from before this, whose message is a title and maybe a trailer, has none,
  and `draft` and `notes` read `docs/releases/` at its commit instead.
- **Written verbatim** (`git tag --cleanup=verbatim`). Git's default cleanup
  takes every `# heading` line out as a comment, even from a message file.
- **Read from GitHub, never the local tag.** On a tag push `actions/checkout`
  replaces the annotated tag with a lightweight one (`+<sha>:refs/tags/TAG`), in
  every job, whatever its `fetch-depth`, so a checkout's local tag has only the
  commit's message. `tag_message` fetches the tag into `FETCH_HEAD`: no ref moves
  and the tree isn't touched. A shallow checkout fetches with `--depth=1`, a full
  one without, since control's build needs its history.
- **Only build jobs stamp.** `chant release publish` refuses a tree with anything
  changed or untracked, so a stamped notes file would stop it just as a stamped
  version would. The publish job's `draft` writes outside the tree.
- **A build also gets earlier releases' notes it lacks.** The pick can be older
  than the commit that put the last release's notes on main, and What's New lists
  every release up to its own. `stamp` writes them from their tags. Tags older
  than the tree's oldest notes are from before there were any.
- **`bump` refuses what would go wrong after the tag.** A PR since the last
  release that the notes neither mention (`#N`) nor list in a `no-notes` block; a
  PR they mention that landed after the commit, which is how 0.66.16's notes went
  wrong; and a headline section `scripts/whats-new.mjs` would refuse, or whose ids
  repeat, which would otherwise fail every build job. Commits that only add notes
  or fixtures' copies don't count, and an app release answers only for PRs that
  touch `crates/desktop`.
- **The stop on newer commits stays** (#1679). `just release` without a commit
  still lists non-green commits and stops unless `--older`; the dry run prints the
  command that names the commit, which holds the pick however main moves.
- **The deploy keeps its route.** `arugula-release` can't pass a file, so it takes
  the notes as the commit has them, under the same checks; they still go in the
  tag. Its plan text is unchanged.

Not doing yet: CI on a chosen commit that isn't green (a `ci/build/**` branch).
Main's one-at-a-time CI yields a green commit about every 30 minutes.

Where: `scripts/release` (`tag_message`, `tag_notes`, `older_notes`, `prs_in`,
`record_to_main`, the `stamp`, `draft`, `notes` and `bump` cases),
`scripts/tests/release-bump.sh`.

From: Arugula native #707.
