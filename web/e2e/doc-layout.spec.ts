// What a Doc's layout is made of, with no browser (Arugula #829): the name a
// state carries; an unknown one is `doc`.

import { expect, test } from "@playwright/test";
import { docLayout } from "../src/shared/docs/layout";

test("a layout this build doesn't know, or none, is a plain Doc", () => {
  expect(docLayout("tabs")).toBe("tabs");
  expect(docLayout("prototype")).toBe("prototype");
  for (const other of [undefined, null, "", "doc", "gallery", "Tabs"]) expect(docLayout(other), String(other)).toBe("doc");
});
