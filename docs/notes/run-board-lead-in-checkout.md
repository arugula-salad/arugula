---
section: Fixed
title: A board's lead starts in its repo
line: Run this board starts the board's lead in its machine's clone of the board's repo, and says so on the card when that machine has none.
headline: no
---
Approving Run this board started the lead in the home folder when the project linked more than one repo, so its first turn went looking for the repo. It now starts in a worktree of the machine's clone of the board's repo: the one its rows' pull requests go to, else another of the project's repos, found where the machine's terminals work or in its home folder. A machine with no clone of any of them doesn't start the lead, and the card says which repo to clone there.
