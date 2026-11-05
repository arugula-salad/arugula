---
section: Changed
title: A box's GitHub token is its own
line: A cloud workspace clones a linked repo with a read-only token of its own that it revokes once cloned, and unlinking a repo revokes Arugula's tokens for it where it can.
headline: no
---
A cloud workspace's setup clones each GitHub repo linked to its project with a
token minted for that box alone, which reads the repo's contents and nothing
else. The box revokes it once its clones are done, and gets no more until
it's rebuilt. Unlinking a repo also revokes these tokens and the one Arugula
keeps for reading the repo, on a best-effort basis: any it can't reach end
within the hour.
