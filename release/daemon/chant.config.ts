// The daemon's release, a v* tag (#1986). .github/workflows/release.yml
// builds the tarballs, the Windows zip and the editor extension into
// dist/release/ (gitignored), cuts this member's release with
// `chant release cut vX.Y.Z --at <the commit CI passed>` and publishes it
// with `chant release publish vX.Y.Z`. A publish never builds: it attaches
// what the build jobs left in dist/release/.
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
// It publishes as latest: releases/latest is always the daemon's. The
// release's title and notes are on the draft `scripts/release draft` makes
// before the publish, which the forge target finds and fills. No SBOM: the
// nearest lockfile chant would write one from is the root's package-lock.json,
// which lists chant and not what the binaries are built from.
//
// The Homebrew formula and the editor marketplaces stay release.yml's own
// steps for now (#1986): chant's homebrew target pushes with a GitHub token
// where the tap has an SSH deploy key (HOMEBREW_TAP_KEY), and its
// marketplace targets take tokens (VSCE_PAT, OVSX_PAT) where release.yml
// publishes with the workflow's OIDC token and no stored secret.
import { readFileSync } from "node:fs";

const tag = process.env.ARUGULA_RELEASE_TAG || "v0.0.0-ARUGULA_RELEASE_TAG-unset";
const version = tag.replace(/^v/, "");

// `just dist`'s targets, a tarball each; scripts/tests/release-targets.sh
// checks this list against the justfile's.
const targets = ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl", "aarch64-apple-darwin", "x86_64-apple-darwin"];

// The extension has its own version; `arugulad _vsix` names the file by it.
const ext = JSON.parse(readFileSync(new URL("../../crates/arugulad-editor/src/editor/ext/package.json", import.meta.url), "utf-8")) as { name: string; version: string };

const dist = "../../dist/release";

export default {
  environments: ["github"],
  release: {
    publish: [
      {
        type: "forge",
        forge: "github",
        repo: "arugula-salad/arugula",
        tag,
        latest: true,
        sbom: false,
        assets: [
          ...targets.map((t) => `${dist}/arugula-${version}-${t}.tar.gz`),
          `${dist}/arugula-${version}-x86_64-pc-windows-msvc.zip`,
          `${dist}/${ext.name}-${ext.version}.vsix`,
          `${dist}/SHA256SUMS`,
        ],
      },
    ],
  },
};
