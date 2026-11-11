---
section: Fixed
title: An agent session says why its branch isn't pushed
line: Reading or waiting on an agent session now shows its box's latest error since its branch was last pushed, such as a push that failed on a full disk, instead of just "not pushed".
needs: control
---
`read_session`, `wait` with `agent_session` and the list of a project's sessions add the error after "not pushed" (or after the last push time). It clears once a later turn pushes the branch.
