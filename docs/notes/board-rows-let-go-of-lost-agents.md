---
section: Fixed
title: Board rows let go of lost agents
line: A row no longer stays in Doing, or claimed, after its agent's machine went quiet or left control, and an agent that closes while control is unreachable still counts as closed.
headline: no
---
A machine now sends each agent block's state again every five minutes, and
with how long ago it saw it. A report older than twenty minutes counts as
unknown: the row follows its other work, and another agent may take it. A
machine that left control holds no row. A person's lane on a row is kept
when control can't read a block's grant for a moment. Machines on an older
Arugula keep working as before.
