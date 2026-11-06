// Control's platform (arugula-platform), released to production with each
// release, from the image the release names (CI builds and pushes
// it: .github/workflows/server-release.yml). It deploys with every release,
// before the hub: a hub never forwards a route the platform doesn't serve yet.
//
// `fly-release` releases platformA and verifies it; platformB takes the same
// image in the same apply (`images`). With `checks`, the apply rolls them one
// at a time, as `fly deploy` did: each must pass its Fly check
// (/platform.json, which answers 503 until the Machine reaches Postgres)
// before the next is updated, and verify needs both passing. A failure puts
// back the config of every Machine the release changed, each waiting for its
// checks (INTENTIUS/chant#4719), as scripts/ci-fly-rollback did.
//
// scripts/ci-server-deploy refuses to deploy it unless the app has the
// ARUGULA_PLATFORM_DATABASE_URL secret (the platform is Postgres only).
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
        checks: true,
      },
    ]),
  ],
};
