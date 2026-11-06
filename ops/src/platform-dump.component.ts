// The platform's nightly dump Machine (packaging/platform-dump around the
// release's arugula-platform), moved onto the release's dump image after the
// platform, in the same job: it runs only once the platform has deployed.
// It deploys with every release, so its
// arugula-platform is the platform's (docs/control-ops.md, "The nightly
// dump"). It's left stopped: Fly starts it every hour.
import { phase, type Component } from "@intentius/chant/components/component";

export const platformDump: Component = {
  name: "platform-dump",
  dependsOn: ["platform"],
  deploy: [
    phase("Image", [{ kind: "release-image" }]),
    phase("Release", [
      {
        kind: "fly-release",
        plan: "dist/platform-dump.fly.json",
        machine: "platformDump",
        image: "@Image.uri",
        digest: "@Image.digest",
      },
    ]),
  ],
};
