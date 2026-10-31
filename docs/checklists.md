# Test checklists in machine Docs

A machine Doc can hold a test checklist in each section. The agent posts
the tests and assigns each one to an agent block on that machine. The
person marks passes, or reports failures with a note or screenshot. The
Doc saves the results and sends failed reports directly to the assigned
agent's durable prompt queue. The lead does not have to relay them.

## Post a checklist

Make a Doc with `doc new`, then use `doc checklist`:

```json
{
  "kind": "checklist",
  "block": 7,
  "section": "s1",
  "items": [
    {
      "id": "phone-layout",
      "text": "The action buttons fit on a phone",
      "owner": { "role": "frontend", "pane": 12 }
    }
  ]
}
```

`block` is the Doc block; `owner.pane` is an agent block on the same
machine, as `list` names it. The calling agent must be allowed to drive
every recipient as well as the Doc; one unauthorized recipient rejects
the entire assignment. `owner.role` is a label the person sees. These
labels belong to the checklist; they do not claim a workspace-wide role
name (#1477). The saved block id survives daemon restarts and agent
resumes. If that block is closed and replaced, call `doc checklist` again
with its replacement id. Existing reports keep the recipient the person
saw when reporting. The person can report again to send to the replacement;
an agent changing a definition cannot silently redirect saved feedback.

Use sections to group tests by area. `doc fill` can add or update a
section's instructions without replacing checklist results. Calling
`doc checklist` again keeps matching items' results. Once an item has
feedback, keep its id and test text; use a new section for a different
test. A Doc has at most 100 items. Section bodies and checklist storage
share the 64 KB section and 1 MiB Doc limits, with room reserved for
delivery receipts. A note is at most 4,000 bytes.

## Test and report

In the owner's live Doc, tick a checkbox for a pass. Choose **Report a
failure**, describe what happened or attach a screenshot, then choose
**Report failure to ROLE**. Screenshots are PNG, JPEG, GIF or WebP, at most
2 MB each. They have a separate budget of 100 referenced images per Doc,
leaving its 20 regular picture slots available for the agent's content.
Identical bytes reuse an immutable image; replacing a screenshot reclaims
its old image unless another test or section still references it. The
verdict's revision and size checks pass before any image is written. A
rejected save creates no uploaded asset; restore also reclaims an image
left by a crash before its verdict was saved. Screenshot storage is
bounded, but a large Doc snapshot may still exceed its sharing quota.
The screenshot is copied into the receiving agent's prompt, so it can
inspect the failure even from another tab or a hosted agent.

The result is saved before delivery. The Doc shows **Saved; waiting to
send** until the receiving agent accepts it, then **Sent to owner**. A busy
agent queues it for its next turn. A retry task exists only while feedback
is pending; if an agent is unavailable, the open Doc retries every two
seconds. Pending results and receipts survive a
restart or closing and reopening the Doc; retrying resumes when its block
is open again.

Marking a previously failed test as passed or untested sends a result
update to the report's original recipient too, so a queued failure does
not remain the agent's last instruction after the person has resolved it.

The recipient's transcript labels these prompts **A follow-up from
Checklist feedback**. The prompt itself identifies the verdict and note
as the person's report, and the Doc title and test text as relayed checklist
context. This context is not presented as the owner's own instructions.

Delivery is at least once. A crash after enqueueing a report but before
saving its receipt may send it again. Each message includes a stable
report id; a repeated id is the same feedback, not a new task. An accepted
report is not sent again after its receipt is saved.

Agents read the person's current results with `read_output` on the Doc.
Its text and copied Markdown include checked passes, failed results,
notes, screenshot references and delivery status. A shared snapshot
includes those results as read-only Markdown. Project Docs on the
platform are a separate store and do not gain checklist editing here.

## Access and concurrency

The owner reports results. A shared viewer or editor, an agent's CLI and
a team's other owner cannot use the feedback endpoint to enqueue work as
the machine's owner. Agents can define checklists in Docs they may drive,
but cannot report a person's verdict with `doc`.

`doc_feedback` is a block method with `section`, `item`, `revision`,
`status` (`todo`, `passed` or `failed`), `note`, and optional `screenshot`
(an asset name already stored in that Doc) or `image` (new base64 image
bytes saved with the verdict; these replace `screenshot` if both are
supplied). Each item has its own saved
revision. A stale device is refused rather than overwriting newer results.
Definitions and feedback live in atomic `sN.tasks.json` sidecars, serialized
with section writes. A corrupt sidecar is reported and left untouched.
