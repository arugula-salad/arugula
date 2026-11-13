// The daemon's release, a v* tag (#1986). .github/workflows/release.yml
// cuts this member's release with `chant release cut vX.Y.Z --at <the tag's
// commit> --lane daemon`, builds the tarballs, the Windows zip and the
// editor extension into dist/release/ (gitignored) and publishes it with
// `chant release publish vX.Y.Z`. A publish never builds: it sends what the
// build jobs left in dist/release/.
//
// The tag's commit is the release's, as CI passed it: no commit moves the
// version. The build jobs write the tag's version into their checkout
// (`scripts/release stamp`) before they build, so the binaries carry it.
// The cut reads the daemon's lane, ci/green-daemon (release.lane).
//
// The GitHub release stays the tag's own, vX.Y.Z, not the tag chant's cut
// makes (release/daemon-release/vX.Y.Z): the daemon's update check,
// install.sh, install.ps1 and the Homebrew formula download from
// releases/download/vX.Y.Z/. A forge target's `tag` is fixed in the
// declaration, so release.yml names the release's tag in ARUGULA_RELEASE_TAG
// and the assets, whose names carry the version, follow from it. Without it
// (a cut, `chant workspace check`) the names are a placeholder no build
// makes, so a publish run without it stops at its plan (publish-file-missing).
//
// Four targets, revealed in this order:
// - forge: the tag's release, published as latest (releases/latest is
//   always the daemon's). Its title and notes are on the draft `scripts/release
//   draft` makes before the publish, which the forge target finds and fills.
//   No SBOM: the nearest lockfile chant would write one from is the root's
//   package-lock.json, which lists chant and not what the binaries are built
//   from.
// - homebrew: Formula/arugula.rb in arugula-salad/homebrew-tap, from the
//   template `scripts/release formula` writes into dist/release/ from
//   packaging/homebrew/arugula.rb.in. It pushes over SSH with the tap's deploy
//   key (HOMEBREW_TAP_KEY), so no token reaches the tap.
// - marketplace (vscode, openvsx): the editor extension as
//   arugula.arugula-editor (#529), with the run's identity and no stored
//   token: the Entra app arugula-marketplace-publish through azure/login for
//   the VS Code Marketplace, and Open VSX's trusted publishing for this
//   workflow. A version that's there already is skipped, so most releases
//   send nothing: bump the extension's package.json to ship it. Each warns
//   instead of failing when its store refuses the upload, as Open VSX has
//   with a 406 since 0.44.0 (#1015), so one store refusing holds back
//   neither the other nor the release.
import { readFileSync } from "node:fs";

const tag = process.env.ARUGULA_RELEASE_TAG || "v0.0.0-ARUGULA_RELEASE_TAG-unset";
const version = tag.replace(/^v/, "");

// `just dist`'s targets, a tarball each; scripts/tests/release-targets.sh
// checks this list against the justfile's.
const targets = ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl", "aarch64-apple-darwin", "x86_64-apple-darwin"];

// The extension has its own version; `arugulad _vsix` names the file by it.
const ext = JSON.parse(readFileSync(new URL("../../crates/arugulad-editor/src/editor/ext/package.json", import.meta.url), "utf-8")) as { name: string; version: string };

const dist = "../../dist/release";

// The formula downloads the four tarballs (no Windows build in Homebrew).
const tarballs = targets.map((t) => `${dist}/arugula-${version}-${t}.tar.gz`);
const vsix = `${dist}/${ext.name}-${ext.version}.vsix`;

export default {
  environments: ["github"],
  release: {
    lane: "daemon",
    publish: [
      {
        type: "forge",
        forge: "github",
        repo: "arugula-salad/arugula",
        tag,
        latest: true,
        sbom: false,
        assets: [...tarballs, `${dist}/arugula-${version}-x86_64-pc-windows-msvc.zip`, vsix, `${dist}/SHA256SUMS`, `${dist}/SHA256SUMS.sig`],
      },
      {
        type: "homebrew",
        tap: "arugula-salad/homebrew-tap",
        formula: "Formula/arugula.rb",
        template: `${dist}/arugula.rb`,
        assets: tarballs,
        url: "https://github.com/arugula-salad/arugula/releases/download/{label}/{name}",
        sshKeyEnv: "HOMEBREW_TAP_KEY",
      },
      { type: "marketplace", marketplace: "vscode", vsix, auth: "azure-credential", onRefusal: "warn" },
      { type: "marketplace", marketplace: "openvsx", vsix, auth: "trusted-publishing", onRefusal: "warn" },
    ],
  },
};
