import type { ChantConfig } from "@intentius/chant";

// Control's hub (illogical-control) and its platform (arugula-platform) on
// Fly, released from the server lane's green commits (#1983). Production
// only for now (lex00, 2026-11-01): no staging, so the prod gate binds the
// plan and the environment.
export default {
  lexicons: ["fly"],
  environments: ["prod"],
  release: { lane: "server" },
  // control's mount names its volume by id (src/fly/control/control.fly.ts).
  lint: { rules: { FLY011: "off" } },
} satisfies ChantConfig;
