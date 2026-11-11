---
section: Changed
title: Session pushes need everyone's GitHub write
line: An agent session pushes its branch only while its asker and the project's owners and editors can all push to the repo on GitHub, and it never changes workflow files.
needs: control
---
An agent session in a thread pushes its branch only while everyone who can
steer it may push to the repo on GitHub themselves. That means its asker, the
project's owners and editors, and anyone whose GitHub review was sent to it. When
one of them hasn't signed in to control with GitHub, or GitHub gives them less than
write, the push doesn't go, and the thread names them and says why. Give them write
on the repo, have them sign in with GitHub, or make them a viewer.

**Workflow files.** A session's push no longer writes `.github/workflows/`. A turn
whose commits change a workflow file isn't pushed, and the thread says which files.
A session's agent is told not to change them and to say in its answer what should
change. A session that began before the repo's default branch changed its workflows
can't push either: ask again in a new session.

**Reviews on a session's PR** go to the session only from someone GitHub says may
push to the repo. Anyone else's review gets a line in the thread, and a person can
pass it on by replying.

**A cloud workspace's `GIT_TOKEN`** is used only on the hosts its repos are on when
the token is set. After adding a repo on another host, set it again with
`arugula cloud secret set <workspace> GIT_TOKEN`. A token set before this release
clones nothing until it's set again.

**Session slots** start empty for each session, apart from build caches. A slot
keeps those only for the same person's next session.
