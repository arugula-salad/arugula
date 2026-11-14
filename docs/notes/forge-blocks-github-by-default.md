---
section: Fixed
title: A pull request or issue that names no forge opens on GitHub
line: A pull request or issue given as OWNER/REPO#N, with no host, link or provider, opens on GitHub again, not Forgejo.
headline: no
---
Opening `owner/repo#N` with nothing to say which forge it's on went to
Forgejo. It's GitHub's, on github.com, again. A Forgejo or GitLab host, a
link's look, a clone's remote or an explicit `provider` still choose theirs.
