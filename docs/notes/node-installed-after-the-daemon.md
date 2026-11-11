---
section: Fixed
title: Node installed after Arugula is found
line: Install Node after Arugula (with nvm, say) and "Use Claude Code with Arugula" finds it, without restarting the daemon.
headline: no
---
The daemon reads your login shell's environment once, when it starts, so a
Node installed afterwards (nvm, fnm, nodejs.org) wasn't on the PATH it gave
agents' adapters: *Use Claude Code with Arugula* and `arugula setup claude`
kept saying "needs Node 20+" and to install the Node that was there. Now,
when the environment it kept has no Node new enough, it reads the shell
again first; an agent block that can't find Node does the same, so starting
it again works.
