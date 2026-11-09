---
section: Added
title: CI alerts as native issues
line: Turn on CI alerts in a project's settings and a red main opens an issue there, chant's culprit and stale events comment on it with a push, and a green commit closes it.
needs: control
---
A project's settings have a CI alerts section. Turning it on shows the URL
chant's tick posts to and a key, shown once, that signs its events; Rotate
key makes a new one, and Turn off removes it.

In the repository, `ci.green.notify` in `chant.workspace.json` names the
two environment variables (`webhook` and `key`), and each is a repository
secret of that name. Then a red branch opens one issue, written by chant CI
rather than by a person or an agent. Red again, the bisect's culprit (with
its author and pull request when the repo is linked here) and a branch still
red after `staleAfter` hours comment on it, and the next green commit
comments and closes it as completed. A red after that opens a new issue.

The people the section names are mentioned on a new red issue, and assigned
it if you choose, and mentioned again on the culprit, the stale reminder and
the green. Their push settings apply.
