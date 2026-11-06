// Control's platform (arugula-platform), released to production with each
// release, from the image the release names (CI builds and pushes
// it: .github/workflows/server-release.yml). It deploys with every release,
// before the hub: a hub never forwards a route the platform doesn't serve yet.
//
// `fly-release` releases platformA and verifies it; platformB takes the same
// image in the same apply (`images`). The apply waits for each to start, one
// after the other, but not for its Fly checks, which `fly deploy` waited for
// (INTENTIUS/chant#4719).
import { phase, type Component } from "@intentius/chant/components/component";

export const platform: Component = {
  name: "platform",
  dependsOn: [],
  deploy: [
    phase("Image", [{ kind: "release-image" }]),
    phase("Release", [
      {
        kind: "fly-release",
        plan: "dist/platform.fly.json",
        machine: "platformA",
        image: "@Image.uri",
        digest: "@Image.digest",
        images: { platformB: "@Image.uri" },
      },
    ]),
  ],
};
