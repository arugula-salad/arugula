// #472, Chromium: see board-activate-cases.ts.
import { boardActivateCases } from "./board-activate-cases";

boardActivateCases("board-activate", (browser, t) => t.person(browser, "alice"));
