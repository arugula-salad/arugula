#!/usr/bin/env bash
# How the workflows hold what releases need (docs/secrets-inventory.md):
# - a job that reads a release, signing or deploy secret names an
#   environment, which only main or protected tags reach; the repository's
#   own secrets are only the compile cache's and the forge tests';
# - what ships compiles from source: release.yml's Linux builds and
#   server-release's build don't get the compile cache's password;
# - every action that isn't this repository's is pinned to a commit, and so
#   is every action the release workflows use.
#
#   scripts/tests/release-workflows.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
if ! python3 -c 'import yaml' 2>/dev/null; then
  echo "release workflows: skipped (no PyYAML here)"
  exit 0
fi
python3 -I - "$root" <<'PY'
import glob, json, re, sys
import yaml

root = sys.argv[1]
repo_level = {"GITHUB_TOKEN", "SCCACHE_WEBDAV_PASSWORD", "ARUGULA_GH_AUTHOR_TOKEN", "ARUGULA_GH_REVIEWER_TOKEN", "ARUGULA_GH_APP_PRIVATE_KEY"}
from_source = {"release.yml": ["linux", "linux-arm64"], "server-release.yml": ["build"]}
release = {"release.yml", "app-release.yml", "server-release.yml"}
sha = re.compile(r"^[^@]+@[0-9a-f]{40}$")
fail = []


def secrets(x):
    return set(re.findall(r"secrets\.([A-Za-z0-9_]+)", json.dumps(x)))


def uses(x):
    if isinstance(x, dict):
        for k, v in x.items():
            if k == "uses" and isinstance(v, str):
                yield v
            else:
                yield from uses(v)
    elif isinstance(x, list):
        for v in x:
            yield from uses(v)


for path in sorted(glob.glob(f"{root}/.github/workflows/*.yml")):
    name = path.rsplit("/", 1)[1]
    w = yaml.safe_load(open(path))
    top = secrets(w.get("env", {}))
    for job_name, job in (w.get("jobs") or {}).items():
        env = job.get("environment")
        held = secrets(job) | top
        if held - repo_level and not env:
            fail.append(f"{name}: {job_name} reads {sorted(held - repo_level)} without an environment")
        if job_name in from_source.get(name, []) and "SCCACHE_WEBDAV_PASSWORD" in held:
            fail.append(f"{name}: {job_name} builds what ships, but gets the compile cache's password")
    for u in uses(w):
        if u.startswith("./"):
            continue
        if (name in release or not u.startswith("actions/")) and not sha.match(u.split(" ")[0]):
            fail.append(f"{name}: {u} isn't pinned to a commit")

for path in sorted(glob.glob(f"{root}/.github/actions/*/action.yml")):
    a = yaml.safe_load(open(path))
    for u in uses(a):
        if not u.startswith("./") and not u.startswith("actions/") and not sha.match(u):
            fail.append(f"{path.split('/.github/')[1]}: {u} isn't pinned to a commit")

for f in fail:
    print(f"FAIL  {f}")
if fail:
    sys.exit(1)
print("release workflows: all passed")
PY
