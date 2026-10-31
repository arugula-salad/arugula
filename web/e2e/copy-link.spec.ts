// The Copy link button (#411) in Chrome: the link is read back from the
// clipboard, and the pages are photographed for the owner (test-results/).
import { copyLinkSpecs } from "./copy-link-steps";

copyLinkSpecs("copylink", true, true);
