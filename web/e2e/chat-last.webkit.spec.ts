// #965 on an iPhone and at a computer's width in WebKit; the cases are in chat-last-cases.ts.
import { test } from "@playwright/test";
import { chatLastTests } from "./chat-last-cases";
import { iphone } from "./phones";

test.describe("phone", () => chatLastTests(iphone, true));
test.describe("computer", () => chatLastTests({}, false));
