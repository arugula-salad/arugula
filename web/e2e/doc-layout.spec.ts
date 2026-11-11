// What a Doc's layout is made of, with no browser (Arugula #829, #830): the
// name a state carries (an unknown one is `doc`), and the one section of a
// prototype Doc as its view draws it: a line and one prototype, and nothing
// else, or a plain Doc.

import { expect, test } from "@playwright/test";
import { docLayout, protoOnly } from "../src/shared/docs/layout";

const FENCE = "```prototype height=300 Timer\n<p>hi</p>\n```";
const one = (body: string | null) => ({ entries: [{ id: "s1", body }] });

test("a layout this build doesn't know, or none, is a plain Doc", () => {
  expect(docLayout("tabs")).toBe("tabs");
  expect(docLayout("prototype")).toBe("prototype");
  for (const other of [undefined, null, "", "doc", "gallery", "Tabs"]) expect(docLayout(other), String(other)).toBe("doc");
});

test("a prototype Doc is one line and one prototype, and anything else is drawn as a plain Doc", () => {
  expect(protoOnly(one(`A timer.\n\n${FENCE}\n`))).toEqual({ line: "A timer.", html: "<p>hi</p>", alt: "height=300 Timer", section: "s1" });
  // Only the prototype: no line under the title.
  expect(protoOnly(one(FENCE))?.line).toBe("");
  // Not that: pending, no fence, two, another kind, or more than one section.
  expect(protoOnly(one(null))).toBeNull();
  expect(protoOnly(one("Just a line."))).toBeNull();
  expect(protoOnly(one(`${FENCE}\n\n${FENCE}`))).toBeNull();
  expect(protoOnly(one("```js\nlet a = 1\n```"))).toBeNull();
  expect(protoOnly({ entries: [] })).toBeNull();
  expect(protoOnly({ entries: [...one(FENCE).entries, { id: "s2", body: FENCE }] })).toBeNull();
});
