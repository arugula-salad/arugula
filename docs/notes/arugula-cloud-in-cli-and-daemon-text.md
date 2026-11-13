---
section: Changed
title: The CLI and agents say "Arugula cloud"
line: The CLI, the daemon's messages and the tools agents read now say Arugula cloud, where they said Arugula control or the platform page.
headline: no
---
The web client moved to plain words in #2134 (#598); the command line, the
desktop app's menu, the daemon's messages and the text agents read follow
(#663). `arugula status` says "Not joined to Arugula cloud", and the help for
`arugula login`, `join`, `pr`, `issue` and the rest says Arugula cloud too.
Settings and what only the web shows are on "your machines page". Routes,
flags, origins and names are unchanged.
