---
section: Added
title: An agent can withdraw or replace its approval card
line: An agent's approval card now comes with a link to answer it, and the agent can replace it with a newer one, withdraw it, or check where it stands.
needs: control
headline: no
---
When an agent on your machine asks you to approve something, it now gets the card's link back and can give it to you in chat, since chat can't approve a card. For a personal project the link goes to your Inbox. If the agent asks again for the same thing it passes the old card's id as `supersedes`, so the old card is withdrawn instead of two cards starting two agents. It can also withdraw a card you no longer want (`withdraw`) and read where one stands and who answered it (`status`). An older Arugula cloud still takes the card, without the replacing.
