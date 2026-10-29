<!-- Codex's buyer research for /hosted (2026-10-29): who buys, what's in their head, ranked selling points, headlines, page copy, words to avoid. The page (src/hosted.tsx) takes its structure and most of its lines from here, with less hedging in the FAQ. -->

# arugula.stream — buyer strategy and marketing page copy

## 1. Who is in a buying mood?

The best prospect already has a reason for people to be in the same place at the same time. They want those people to notice each other. Presence cannot create a community, a class, or a useful collaboration workflow on its own.

### A. The collaborative SaaS founder or product lead

- **Their product:** A shared document editor, design workspace, whiteboard, or project tool.
- **The trigger:** In a product review or customer conversation, someone asks, “How do I know my teammate is here?” The product supports shared work, but the experience still feels solitary.
- **What they've considered:** Online dots, avatar stacks, live cursors, chat, and sending everyone to a separate meeting link.
- **Buying intent:** “Give us a more human sense of being together without turning our product into a conferencing tool.”
- **What wins them over:** A visible, understandable interaction: a teammate looks up, their character moves, and someone waves back. A place in the existing workflow matters more than a long feature list.

### B. The founder of a coworking, community, or cohort product

- **Their product:** A virtual coworking space, focus app, remote-team hub, or learning community with scheduled sessions.
- **The trigger:** People arrive, keep cameras off, and disappear into a list of names. Hosts have to keep asking who's still there.
- **What they've considered:** Always-on video rooms, chat channels, scheduled check-ins, reaction buttons, and standalone virtual offices.
- **Buying intent:** “Let people feel accompanied without asking them to broadcast their face and room.”
- **What wins them over:** Small signs of life that fit quiet work and learning. The important qualification: these live characters still need webcam input. They are not a solution for someone who refuses to use a camera at all.

### C. The product lead for a customer workspace

- **Their product:** A client portal, onboarding workspace, or customer-facing app where staff and customers sometimes work together.
- **The trigger:** During a shared session, customers cannot tell that a helpful person is present. The team wants a friendlier way to invite a first interaction.
- **What they've considered:** Chat widgets, meeting links, support pop-ups, and building a staff-presence indicator.
- **Buying intent:** “Make the people behind our product visible when they're actually here.”
- **What wins them over:** Presence and a wave as a small opening gesture. This is a narrower fit today: calls are next, and the supplied product facts do not include help queues, routing, or support messaging. Do not sell it as a complete live-help system.

### D. The engineering lead evaluating build versus buy

- **Their product:** Any of the above, with a product sponsor already asking for richer presence.
- **The trigger:** “Could we ship something like this?” becomes a real roadmap discussion. Someone now has to estimate webcam tracking, realtime transport, auth integration, and ongoing operations.
- **What they've considered:** Building in-house, stitching together libraries and a realtime service, or operating an open-source relay themselves.
- **Buying intent:** “Give the product team this experience without giving my team another service to operate.”
- **What wins them over:** Hosted realtime infrastructure, existing sign-in, a small frontend integration, and an open-source client, protocol, and relay. They will also need candid answers about early-stage reliability and support before approving a rollout.

**Primary page audience:** A founder or PM buying a more human shared experience. Engineering is the essential evaluator. Lead with what users feel; give engineering enough evidence to investigate.

## 2. What's in their head?

### The job they're hiring this for

“When people share a workspace, help them notice and acknowledge each other, so being in our product feels like being somewhere together.”

The first purchase is presence: noticing, nodding, waving, reacting. Voice and video are a future extension of that job, not a current reason to promise a complete solution.

### The feeling they want users to have

- “Oh, you're here too.”
- “I can say hello without interrupting the whole room.”
- “There's a person behind that name.”
- “I can show a little expression without showing my actual face or background.”

### The objections behind the buying questions

| What they ask | What they're protecting | The honest response |
| --- | --- | --- |
| “Is this creepy?” | User trust and personal boundaries. | The webcam picture stays in the browser. Local tracking sends pose data instead. That is a specific privacy benefit, not a claim that no data leaves the device. |
| “Nobody turns their camera on.” | Adoption. | Users share a character or face drawing instead of camera footage. Webcam input is still required for that live expression; some users will still decline. |
| “Won't this cause camera fatigue?” | Users' attention and comfort. | It changes what people show each other. It does not prove that people feel less watched or less tired. Start with a context where users want company and evaluate their comfort. |
| “Is this just a gimmick?” | Product focus and credibility. | Anchor it to a real moment: noticing a collaborator, acknowledging a classmate, or greeting a coworker. Whether it improves engagement or retention is unproven. |
| “How much will we have to build?” | Engineering time and operational load. | Integration uses a few lines of frontend code and existing sign-in. arugula.stream runs the realtime infrastructure. Integration and product decisions still take work. |
| “Will the bill run away?” | Budget predictability. | Monthly active users provide a usage measure the team can plan around. Exact MAU definitions, allowances, and overage terms must be clear before a commitment. Calls will add minute-based pricing later. |
| “Can we leave?” | Future negotiating power and technical independence. | The client, protocol, and relay are open source. There is a path to running the open-source components yourself, with migration and operational work to assess. |
| “Can we trust a brand-new vendor?” | Continuity and accountability. | Early access is not open yet. There are no customers, testimonials, or usage results to cite. Evaluate the implementation, discuss support and reliability, and choose a limited first use case. |

### The internal pitch

**From the product sponsor:**

> Our shared workspace still makes people guess whether anyone is there. arugula.stream could give them visible signs of each other and a simple way to say hello. The webcam picture stays local. We should evaluate it in one shared workflow and see whether people use it and feel comfortable. We are not assuming a retention lift, and calls are not available yet.

**From the engineering lead:**

> We can integrate with our existing sign-in while arugula.stream operates the realtime infrastructure. The client, protocol, and relay are open source, which gives us an alternative to staying on the hosted service. Before rollout, we need to validate the integration, usage costs, reliability, and support expectations.

## 3. Selling points, ranked

Each headline is a buyer benefit; its supporting sentence stays within the supplied product facts.

1. **Help people feel other people are here.** Live characters reflect webcam-tracked movements and expressions such as nods, blinks, laughter, waves, and looking away; users can also appear as a live line drawing of their face.
2. **Bring expression into your app without broadcasting camera footage.** Face and pose tracking run locally, and only approximately 20 bytes of pose data per frame are sent instead of the webcam picture.
3. **Give people a small way to say hello.** Users can wave at each other and send emoji reactions that float above a person's tile.
4. **Add presence without another realtime service to run.** The hosted service runs the realtime infrastructure, with a few lines of frontend code and support for existing Clerk, Auth0, Firebase, or Supabase sign-in.
5. **Keep a way out.** The client, protocol, and relay are open source, so the hosted service is not the only way to run the technology.
6. **Plan around the people using it.** Hosted pricing is based on monthly active users, with a free tier, an approximately $29/month team tier, and enterprise pricing.
7. **Next: let a hello become a conversation.** Voice and video calls in the same room are planned, with call-minute charges later; calls have not shipped.

## 4. Headline and subhead options

### Twelve headlines

1. **TOP 1 — Make your app feel like someone's here.** Emotional, concrete, and broad enough for the primary audiences; it names the change the buyer wants.
2. **TOP 2 — Bring people into view. Keep camera footage private.** Connects the visible experience to the strongest factual differentiator; “camera footage” is more precise than a blanket privacy claim.
3. **TOP 3 — A little wave. A more human app.** Makes the experience easy to picture and keeps the promise modest; pair it with a descriptive subhead.
4. Your users share a workspace. Help them feel it.
5. Give “who's here?” a human answer.
6. More than a name in the corner.
7. Let people show up as themselves. In character.
8. Give your users someone to wave to.
9. An online dot can only say so much.
10. Being together shouldn't require broadcasting your face.
11. Put a little life in your shared workspace.
12. You built the place. Help people notice each other.

### Six subheads

1. **TOP 1 —** Add live characters that nod, blink, and wave with the people behind them. Help users notice each other and say hello, while their webcam pictures stay in their browsers. **Why:** Explains the mechanism and emotional benefit without leading with implementation.
2. **TOP 2 —** Make shared work feel shared. arugula.stream brings live characters, waves, and emoji reactions into your product. We run the realtime infrastructure. **Why:** Best for collaborative SaaS; connects the experience to the buying convenience.
3. **TOP 3 —** A nod from a teammate. A wave from a classmate. A familiar face, drawn live. Bring small human moments into your app without sharing webcam footage. **Why:** Helps community and learning buyers picture actual use.
4. Give people a visible way to be there, with a live character or a line drawing of their face. Their camera picture stays local. Their presence becomes part of your app.
5. For the moments between meetings: noticing who's around, saying hello, and sharing a reaction. Live presence for your product, hosted by arugula.stream.
6. Start with a wave. Next, a conversation. Add expressive presence to your app, with voice and video calls planned for the same room.

**Recommended hero:** Headline 1 with subhead 1. The privacy-focused headline is a useful alternative when privacy objections dominate buyer conversations. Subhead 6 belongs near the roadmap unless calls become available.

## 5. Full page outline and final copy

Everything below is proposed public page copy through the final CTA. Section labels describe placement. Pricing is presented as a preview, and the CTA requests future access rather than implying immediate entry.

### Navigation

**arugula.stream**

The experience · Privacy · Pricing · FAQ

**Request early access**

### Hero

*Live presence for your product*

# Make your app feel like someone's here.

Add live characters that nod, blink, and wave with the people behind them. Help users notice each other and say hello, while their webcam pictures stay in their browsers.

**Request early access**

Secondary link: **See how it feels**

Early access is not open yet. Request access for when it opens.

### The problem

## A shared workspace can still feel empty.

The document is open. The room has names in it. But is anyone actually here?

An online dot tells you someone is connected. It doesn't give you a nod, a smile, or a wave back.

And when people would rather keep their face and background off-screen, another video meeting may not be the answer.

Give them a little more of each other.

### How it feels

## Small signs of life. Right where people already are.

**A character with a person behind it.**

They nod. Their character nods. They blink, laugh, or look away. It moves with them. Or they can appear as a live line drawing of their face.

**A hello that starts with a wave.**

Wave to someone and your character waves on their screen. A small way to say, “Hey, I'm here too.”

**A reaction with a little lift.**

Send an emoji and watch it float above their tile. A little applause. A shared laugh. A moment of recognition.

### Privacy as a feature

## Share your expression. Keep your camera picture to yourself.

Your webcam picture never leaves your browser.

arugula.stream tracks face and pose locally, then sends a tiny amount of pose data—approximately 20 bytes per frame—to animate what others see.

They see your live character or face drawing. They don't receive your webcam picture.

Live expression still needs webcam input. The difference is what you share.

### What the buyer doesn't have to build or run

## You build the place. We run the live presence.

Give your team a way to add this experience without building and operating the realtime infrastructure behind it.

arugula.stream hosts that infrastructure. Your product keeps its existing sign-in. Your users get a new way to notice each other.

The client, protocol, and relay are open source, so you can inspect the technology and have a path beyond our hosting.

#### For your developers

A few lines of frontend code. Your existing Clerk, Auth0, Firebase, or Supabase sign-in. No presence servers for your team to run.

Conceptual integration, not an SDK signature: `connect({ user, room })`

### Presence that turns into a conversation

*Next · Not available yet*

## Start with a wave. Stay for a conversation.

Sometimes a hello is enough. Sometimes there's more to say.

Next, we're building a way for a wave to become a voice or video call in the same room.

Live characters, face drawings, waves, and emoji reactions are built. Calls have not shipped. Call minutes will be priced separately when calls arrive.

### Use cases

## Wherever people have a reason to be together.

**Shared documents, design tools, and whiteboards**

Let collaborators notice the person beside the work. A nod or wave adds a human moment to a shared task.

**Coworking, focus, and remote-team spaces**

Give people a way to show they're around without sharing live camera footage. Quiet work can still have company.

**Classes, cohorts, and communities**

Give classmates and members a way to acknowledge each other. A wave hello. An emoji for a small win.

**Customer workspaces**

When your team and a customer are in the app together, make that presence visible. Give them a small way to open an interaction.

### Pricing

## Price presence around the people who use it.

Hosted pricing is based on monthly active users. That gives you a familiar starting point for planning: how many people will use presence each month?

Plan for a smaller rollout first, then account for more active users as you expand. Your bill can change with usage; MAU pricing does not mean a fixed bill.

*Pricing preview · Early access is not open yet*

| Plan | Price preview | Where it fits |
| --- | --- | --- |
| Free | Free tier | A starting point for trying presence when access opens. |
| Team | Approximately $29/month | A paid starting point for a team adding presence to its product. |
| Enterprise | Discuss pricing | For organizations evaluating their requirements and usage. |

Included MAUs, counting rules, and overage terms are still to be confirmed. Use these prices as a preview, not a final quote. Voice and video calls are next and will add call-minute charges later.

**Request early access**

### FAQ

## A few fair questions.

**Will people actually use this?**

We don't have customer results to share yet. The clearest place to evaluate it is a workflow where people already want company or collaboration. Look for whether they notice each other, wave, react, and want to keep using it. Don't assume a retention lift.

**Does everyone need a webcam?**

The live character and face drawing use webcam input to follow a person's movements and expressions. They let people show expression without sharing their camera picture. They won't solve every reason someone prefers to keep a camera off.

**Is this another way to feel watched?**

It may not suit every person or every setting. Other people can see the movements and expressions reflected in your character or drawing. They cannot see your webcam picture. Choose a use case where that shared presence is welcome.

**What leaves the browser?**

For the live animation, approximately 20 bytes of pose data per frame. Face and pose tracking run locally. The webcam picture stays in the user's browser.

**Does the same privacy claim apply to future video calls?**

The local-camera claim describes live presence. Calls have not shipped. Don't assume future video calls will work the same way; their media and privacy details need to be evaluated separately when available.

**Do we need to build a backend for this?**

No presence backend. The hosted service runs the realtime infrastructure. You add a few lines of frontend code and connect your existing Clerk, Auth0, Firebase, or Supabase sign-in.

**Can a wave start a call today?**

No. Waves and emoji reactions are built. Turning a wave into a voice or video call in the same room is next. There is no launch date to promise here.

**How predictable is the price?**

Monthly active users give you a basis for estimating usage. The total can rise as more people use presence. The pricing preview is a free tier, a team tier around $29/month, and enterprise pricing. Confirm included usage, the MAU definition, and overage terms before budgeting. Calls will add minute-based charges later.

**Are we locked into your hosting?**

The client, protocol, and relay are open source. You can evaluate running those components yourself. Moving away from hosting would still involve technical and operational work.

**How established is arugula.stream?**

It's early. Access is not open yet, and we don't have customers or testimonials to point to. If the experience fits your product, request early access and start a conversation about what you would need to evaluate it.

### Final CTA

## Give “I'm here” a little life.

A nod. A wave. A small reaction that makes a shared space feel shared.

If that belongs in your product, we'd like to hear what you're building.

**Request early access**

Early access is not open yet. Requesting access does not provide immediate entry. Calls are next, not available today.

---

## 6. Words and claims to avoid

| Avoid | Why | Prefer |
| --- | --- | --- |
| “Revolutionize,” “synergy,” “leverage,” “next-generation” | They substitute excitement for an understandable benefit. | “Help people notice each other.” |
| “Seamless,” “effortless,” “zero work,” “instant integration” | Even a small integration requires product and engineering decisions. | “A few lines of frontend code.” |
| “Frontend code only” as the headline | It answers the evaluator's implementation question before the buyer's motivation. | Keep it in the small developer section. |
| “Cameras off,” “camera-free,” “no camera needed” | The live character and face drawing need webcam input. | “Without sharing webcam footage.” |
| “Nobody feels watched,” “end camera fatigue” | Local processing doesn't establish how users feel. | Describe what others see and what stays local. |
| “No data leaves your device,” “zero data,” “completely private” | Pose data is transmitted; footage privacy is a narrower claim. | “Your webcam picture stays in your browser.” |
| “Anonymous,” “untraceable,” “end-to-end encrypted,” “compliant” | None of these properties or certifications is established in the supplied facts. | State the local tracking and pose transmission facts. |
| “Only 20 bytes” without “approximately,” “pose data,” and “per frame” | It can be mistaken for total network usage or an exact bandwidth guarantee. | “Approximately 20 bytes of pose data per frame.” |
| “Boost retention,” “increase engagement,” “reduce churn” | There are no measured outcomes or customers yet. | Present these as hypotheses to evaluate, not product results. |
| “Trusted by,” “teams love,” fabricated quotes or logo strips | No social proof exists. | Show the actual experience, clearly labeled if demonstrated. |
| “Presence and calls,” “wave to call,” “all-in-one communication” without a status label | They imply shipped calling functionality. | “Next: voice and video calls in the same room.” |
| “Get started,” “start free,” “try it now,” “join today” | Access is not open. | “Request early access.” |
| “Limited spots,” “last chance,” “don't miss out” | No scarcity or deadline has been established. | A plain invitation to request access. |
| “Unlimited,” “flat price,” “no surprise bills,” a firm “$29” | Usage limits and overage rules are unspecified; the team price is approximate. | “Pricing preview,” “approximately $29/month,” and an explicit usage caveat. |
| “Enterprise-grade,” “battle-tested,” “production-proven” | Reliability, scale, and enterprise capabilities have not been established. | “Hosted realtime infrastructure,” with evaluation details discussed directly. |
| “Zero lock-in,” “leave instantly” | Open source provides an alternative, not a promise of a costless migration. | “Open-source client, protocol, and relay.” |
| “Metaverse,” “digital humans,” “AI avatars” | They distract from a straightforward, webcam-driven experience; AI claims are not supplied. | “Live characters” and “live face drawings.” |
| “Replace Zoom/Slack/support” | The product is not established as a complete replacement, and calls are not shipped. | Name the specific interaction: notice, wave, react. |

## Publication notes

- Use an actual product demonstration as the page's visual proof. Label staged scenes as demos. Do not imply the pictured participants are customers.
- Keep the secondary hero link on-page, pointing to the experience section or demonstration.
- Connect the early-access CTA to a working request flow before publishing. Do not label submission as account creation or grant access that is not available.
- The code hint is intentionally conceptual. Replace it with one verified SDK line before publishing, or omit it if no stable public API exists.
- The exact MAU definition, included usage, overages, team price, and enterprise terms were not supplied. Confirm these before accepting payment; the preview copy does not invent them.
- Privacy copy applies to the current presence feature. Revisit it when calling ships rather than extending the claim to video media by implication.
