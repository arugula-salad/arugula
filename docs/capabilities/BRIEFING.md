# The Monday briefing

Instructions for the agent that runs this each week, on a fresh checkout of
main. The reader is Jake, who directs the agents and doesn't read most diffs.
Your job is to tell him what's no longer true, and to keep the feature records
honest. Don't write a summary of the week: the release notes already do that.

## 1. Gather

- `docs/capabilities/briefed-through` holds the commit the last briefing
  covered. This week's range runs from there to `HEAD`.
- Run `just map --since "<that commit's date>"`.
- Read the merged PRs' bodies in the range (`gh pr list -s merged`, or
  `git log` for the numbers). The intent lives there, not in the diff.
- Read the release notes in the range (`docs/releases/`).

## 2. Keep the records

For each record `just map` shows as behind, read what changed on its paths
since `verified_through`, and check the record against the code as it is now.

- If it's still right, move `verified_through` to `HEAD`.
- If it's wrong, fix it, then move `verified_through` to `HEAD`.
- If you can't tell, leave `verified_through` where it is and say why under
  "Not known". Never move the commit forward on something you didn't check.

Then deal with what's gone (`gone:` in the map). A record that names a file
that isn't there is wrong: find where the code went.

Add a record when a new feature is big enough that Jake would ask where it
lives. Don't aim for full coverage. Check a feature's purpose and its promises
with a person: mark them unanswered under "Not known" and raise them in the
briefing.

Run `just decisions`, then open one PR with the record changes and
`briefed-through` set to the range's end. Don't touch anything else.

## 3. Write the briefing

Make a new Doc titled "Monday briefing, YYYY-MM-DD", readable in five minutes.
Link the commit range at the top.

**What changed, at most three items.** Each is a belief Jake could have held
last week that's no longer safe:

> You could assume X. Now Y. So Z. (links to the code and the PR)

- Label anything you didn't confirm in the code "unverified".
- Leave out what doesn't change how he should think, however large the PR was.
- Fewer than three is fine. "Nothing changed what you need to know this week"
  is a good briefing.

**One cost, with a choice.** Pick the one cost from `just map` that moved most
or matters most, and say what it costs in its own units: hours main was red,
runner-hours or commits per week. Don't convert them into a single number.
Don't estimate Jake's time: nothing here measures it. Offer fix, defer with a
trigger, or retire, and recommend one. Leave out any cost with no possible
action.

**Open questions.** Include any of these you found:

- what you couldn't verify
- a record's unanswered question that needs a person
- a trap someone learned the hard way that's only in a private note or a
  thread, and belongs in the repo

Post the Doc's link to Jake when it's done.

## 4. Finish

Only `briefed-through` moving means the week was covered, so a failed run
leaves it where it was and next week's range includes this week. Say so if a
step failed.

## The monthly check

On the first Monday of the month, add one question to the briefing: a real
situation, such as a daemon older than the page or a machine asleep. Jake
predicts the outcome before an agent runs the case. Also pick ten claims from
that month's records and briefings at random, check each against the code, and
report how many held.
