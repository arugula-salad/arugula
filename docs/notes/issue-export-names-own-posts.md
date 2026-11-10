---
section: Fixed
title: An issue export names who wrote each comment
line: A Markdown issue export names Arugula's own posts as Arugula, a session's as that session and GitHub's as "<login> on GitHub", not "a deleted account".
headline: no
needs: control
---
Comments nobody here wrote (a session's card, "CI failed …", what a session
said, a comment from GitHub on an issue opened there) are named as the
issue's page names them. "a deleted account" is left for someone who deleted
theirs. The JSON export keeps each comment's raw `via` beside its `author`.
