// #497 in Chromium (pop-out-steps.ts).
import { test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { popOutSpecs } from "./pop-out-steps";

test.afterAll(closeContexts);
popOutSpecs("chrome");
