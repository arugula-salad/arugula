---
section: Added
title: Board leads start agents you can see
line: A board's lead starts one agent block per ready row, each leading its issue's thread, instead of working through sub-agents nobody sees.
needs: control
---
A board's lead now runs standing instructions of Arugula's own (`board-lead`). It starts an agent block for each ready row, on the row and in a tab of its own, with settings that don't ask a permission card for each command. It talks to each agent in its issue's thread, merges green work by the project's rules, and keeps each row's note as a one-line status. Every agent started on an issue is told it leads that issue's thread, and that posts there are your word or its lead's. Start a lead on your machine with `start_agent {recipe: "board-lead"}`, naming the board in its prompt.
