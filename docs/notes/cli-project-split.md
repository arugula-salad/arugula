---
section: Added
title: Split a project from the CLI and MCP
line: `arugula project split` moves repos, boards, issues and Docs into another project, showing the plan first; MCP's `project_split` plans it.
needs: control
---
`arugula project split --name NAME` (with `--owner` a team) moves work into a
new project, and `--into PROJECT` into one that exists. Name what moves with
`--repo`, `--board`, `--issue` and `--doc`, each as often as you like. Without
`--yes` it only plans and changes nothing: what to deal with before you move,
then what comes along with what you named. With `--yes` it moves, and says
where the work went and each issue's new number.

Agents plan the same in MCP's `project_split`, and never move it: the plan
ends with the `arugula project split ... --yes` command for you to run, or
use Move to project… in the project's settings.
