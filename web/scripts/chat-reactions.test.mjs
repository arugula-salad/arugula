// Chat's reactions (#104): a chip's tooltip names who reacted ("You" first,
// agents as "<owner>'s agent on <machine>"), and a reaction added or taken
// back shows at once. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { peopleOf, QUICK, reacted, tip } from "../src/shared/chat/reactions.ts";

const who = (account, name, agent = null) => ({ account, name, agent });
const msg = (reactions) => ({ seq: 1, reactions });

test("the quick set is the six the issue names", () => {
  assert.deepEqual(QUICK, ["👍", "❤️", "😂", "🎉", "👀", "✅"]);
});

test("a tooltip says who, you first, and how many more", () => {
  const c = { emoji: "👍", count: 2, mine: true, who: [who("b", "Sam"), who("a", "Jake")] };
  assert.equal(tip(c, "a"), "You and Sam reacted with 👍");
  assert.equal(tip(c, "z"), "Sam and Jake reacted with 👍");
  assert.equal(tip({ ...c, count: 5 }, "a"), "You, Sam and 3 more reacted with 👍");
  const agent = { emoji: "👀", count: 1, mine: false, who: [who("a", "Jake", { machine: "m", machine_name: "geek" })] };
  assert.equal(tip(agent, "a"), "Your agent on geek reacted with 👀");
  assert.equal(tip(agent, "b"), "Jake's agent on geek reacted with 👀");
});

test("a tooltip names people before agents, and the chip counts them apart", () => {
  const c = { emoji: "👀", count: 3, agents: 1, mine: false, who: [who("a", "Jake", { machine: "m", machine_name: "geek" }), who("b", "Sam"), who("c", "Al")] };
  assert.equal(tip(c, "z"), "Sam, Al and Jake's agent on geek reacted with 👀");
  assert.equal(peopleOf(c), 2);
  assert.equal(peopleOf({ ...c, agents: undefined }), 3, "an older control sends none");
  const by = reacted(msg([{ emoji: "👀", count: 1, agents: 0, mine: false, who: [who("b", "Sam")] }]), "👀", true, "a", "m").reactions[0];
  assert.deepEqual([by.count, by.agents], [2, 1], "an agent's own is an agent's");
  assert.deepEqual([reacted(msg(undefined), "👀", true, "a", "m").reactions[0].agents, reacted(msg(undefined), "👀", true, "a").reactions[0].agents], [1, 0]);
});

test("adding and taking back show before the server answers", () => {
  const on = reacted(msg(undefined), "🎉", true, "a");
  assert.deepEqual(on.reactions.map((c) => [c.emoji, c.count, c.mine]), [["🎉", 1, true]]);
  const twice = reacted(on, "🎉", true, "a");
  assert.equal(twice.reactions[0].count, 1, "once is as good as twice");
  const other = reacted(msg([{ emoji: "🎉", count: 1, mine: false, who: [who("b", "Sam")] }]), "🎉", true, "a");
  assert.deepEqual([other.reactions[0].count, other.reactions[0].mine], [2, true]);
  const back = reacted(other, "🎉", false, "a");
  assert.deepEqual([back.reactions[0].count, back.reactions[0].mine, back.reactions[0].who.length], [1, false, 1]);
  assert.deepEqual(reacted(on, "🎉", false, "a").reactions, [], "the last one takes the chip with it");
});
