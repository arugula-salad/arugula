---
section: Added
title: A pane shows what its agent waits on you for
line: An agent that added a to-do or asked for approval on a card shows "Waits on you" on its pane when its turn ends, until you answer the card.
needs: control
---
An agent on your machine (an agent block, or Claude Code in a terminal with Arugula's hooks) that adds a to-do or raises an approval card now marks its pane as needing you once its turn ends, instead of "Turn finished". The pane names the oldest open card ("Waits on you: Run the migration (+1 more)"). It goes as soon as the answer reaches the machine or the agent cancels the card. It isn't pushed again, because the card already was, and it isn't a second item in your Inbox. Dismissing it hides it until the agent raises another card.
