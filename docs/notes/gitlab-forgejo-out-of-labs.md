---
section: Changed
title: GitLab and Forgejo blocks are on for everyone
line: GitLab merge requests and Forgejo pull requests and issues open as blocks with no Labs flag, beside GitHub's, and their webhooks need no flag either.
---
The `additional-forges` flag (once `forges`) is retired: a link to a GitLab
merge request or a Forgejo pull request or issue opens as a block on every
machine, `capabilities` lists your `tea` and `glab` logins, and the
Forgejo and GitLab webhook routes take signed deliveries. Either name left in
`flags.json` or a team's Labs does nothing.
