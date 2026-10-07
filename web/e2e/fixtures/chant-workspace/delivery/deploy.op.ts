// A third toy gated op, for Run op on the member card (#309): its gate is
// fresh for that test, whatever the others left approved.
import { Op, phase, gate, shell } from "@intentius/chant/op";

export default Op({
  name: "deploy",
  overview: "Wait for a person, then deploy",
  phases: [
    phase("Approve", [gate("approve-deploy", { timeout: "24h", description: "Deploy the toy" })]),
    phase("Deploy", [shell("echo deployed")]),
  ],
});
