import type { ChantConfig } from "@intentius/chant";

// Arugula's ops (src/*.op.ts) only run scripts: no lexicons.
export default {
  lexicons: [],
} satisfies ChantConfig;
