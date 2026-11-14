---
section: Added
title: Split a project from the CLI and MCP
line: `arugula project split` and MCP's `project_split` move repos, boards, issues and Docs into another project, showing the plan first.
needs: control
---
`arugula project split --name NAME` (with `--owner` a team) moves work into a
new project, and `--into PROJECT` into one that exists. Name what moves with
`--repo`, `--board`, `--issue` and `--doc`, each as often as you like. Without
`--yes` it only plans and changes nothing: what to deal with before you move,
then what comes along with what you named. With `--yes` it moves, and says
where the work went and each issue's new number.

Agents have the same in MCP's `project_split`: it plans unless `confirm` is
true, so they can show you the plan before anything moves.
