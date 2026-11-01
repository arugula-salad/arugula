// The desktop app's release, an app-v* tag (#393, #1986).
// .github/workflows/app-release.yml builds the apps into dist/release/
// (gitignored), cuts this member's release with
// `chant release cut app-vX.Y.Z --at <the commit CI passed>` and publishes it
// with `chant release publish app-vX.Y.Z`. A publish never builds.
//
// Two targets, revealed in this order:
// - forge: the tag's own GitHub release, app-vX.Y.Z, with every download,
//   the updater's bundles and signatures, and SHA256SUMS. Never latest:
//   releases/latest is the daemon's (install.sh, install.ps1 and the
//   daemon's update check read it).
// - tauri-updater: the rolling app-latest release, where the site's download
//   links and the updater's endpoint (crates/desktop/tauri.conf.json) point,
//   replaced with this version's files and latest.json. Each platform's URL
//   in latest.json is its bundle on app-vX.Y.Z, which never changes. Moving
//   app-latest's tag takes TAG_TOKEN (CI_GREEN_TOKEN): the workflow's
//   GITHUB_TOKEN gets 403 there (#526).
//
// A forge target's `tag` is fixed in the declaration, so app-release.yml
// names the release's tag in ARUGULA_RELEASE_TAG. Without it the tag is a
// placeholder that names no release. No SBOM, for the reason
// release/daemon/chant.config.ts gives.

const tag = process.env.ARUGULA_RELEASE_TAG || "app-v0.0.0-ARUGULA_RELEASE_TAG-unset";

const dist = "../../dist/release";
const at = (f: string) => `${dist}/${f}`;

// What the updater installs on each platform, signed with the updater key
// (TAURI_SIGNING_PRIVATE_KEY): the .sig beside each is its signature.
const bundles: Record<string, string> = {
  "darwin-aarch64": "arugula-desktop-macos-arm64.app.tar.gz",
  "darwin-x86_64": "arugula-desktop-macos-x86_64.app.tar.gz",
  "linux-x86_64": "arugula-desktop-linux-x86_64.AppImage",
  "linux-aarch64": "arugula-desktop-linux-aarch64.AppImage",
  "windows-x86_64": "arugula-desktop-windows-x86_64-setup.exe",
};

// Every download (scripts/release's app_downloads, which
// scripts/tests/release-targets.sh checks this list against).
const downloads = [
  "arugula-desktop-linux-x86_64.deb",
  "arugula-desktop-linux-x86_64.rpm",
  "arugula-desktop-linux-x86_64.AppImage",
  "arugula-desktop-linux-aarch64.deb",
  "arugula-desktop-linux-aarch64.rpm",
  "arugula-desktop-linux-aarch64.AppImage",
  "arugula-desktop-macos-arm64.zip",
  "arugula-desktop-macos-arm64.dmg",
  "arugula-desktop-macos-x86_64.zip",
  "arugula-desktop-macos-x86_64.dmg",
  "arugula-desktop-windows-x86_64-setup.exe",
];

const updater = Object.values(bundles);
const signatures = updater.map((b) => `${b}.sig`);
// Everything the build makes, once each, and SHA256SUMS over it.
const all = [...new Set([...downloads, ...updater, ...signatures])];

export default {
  environments: ["github"],
  release: {
    publish: [
      {
        type: "forge",
        forge: "github",
        repo: "arugula-salad/arugula",
        tag,
        latest: false,
        sbom: false,
        assets: [...all, "SHA256SUMS"].map(at),
      },
      {
        type: "tauri-updater",
        repo: "arugula-salad/arugula",
        tag: "app-latest",
        platforms: Object.fromEntries(Object.entries(bundles).map(([p, b]) => [p, at(b)])),
        // The bundles are the platforms'; the rest of what the build makes.
        assets: [...all.filter((f) => !updater.includes(f)), "SHA256SUMS"].map(at),
        url: "https://github.com/arugula-salad/arugula/releases/download/{label}/{name}",
        latest: false,
        tokenEnv: "TAG_TOKEN",
      },
    ],
  },
};
