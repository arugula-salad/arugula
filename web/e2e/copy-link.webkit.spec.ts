// The Copy link button (#411) in WebKit: the clipboard can't be read in a test, so
// the button's `data-link` and its "Copied ✓" stand in (copy-link-steps.ts).
import { copyLinkSpecs } from "./copy-link-steps";

copyLinkSpecs("copylink-webkit", false);
