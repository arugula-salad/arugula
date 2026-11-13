// #965 on a Pixel 7 and at a computer's width in Chrome; the cases are in chat-last-cases.ts.
import { test } from "@playwright/test";
import { chatLastTests } from "./chat-last-cases";
import { pixel7 } from "./phones";

test.describe("phone", () => chatLastTests(pixel7, true));
test.describe("computer", () => chatLastTests({}, false));
