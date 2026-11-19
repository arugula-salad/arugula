---
section: Fixed
title: A PR named OWNER/REPO#N opens from GitHub
line: A pull request or issue named as OWNER/REPO#N outside a clone opens from GitHub, and one that can't be read says which forge it tried instead of asking for tea.
headline: no
---
Forgejo is used only when a clone's remote says so. When a pull request or issue can't be read, its block names the forge and item it tried, such as `GitHub: arugula-salad/arugula#2494: …`.
