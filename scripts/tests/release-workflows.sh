#!/usr/bin/env bash
# How the workflows hold what releases need (docs/secrets-inventory.md):
# - a job that reads a release, signing or deploy secret names an
#   environment, which only main or protected tags reach; the repository's
#   own secrets are only the compile cache's read-only login and the forge
#   tests'. The cache's write login is the ci-cache environment's: a
#   workflow's env may name it, since a job that doesn't name the environment
#   gets it empty;
# - what ships compiles from source: release.yml's Linux builds don't get
#   the compile cache's passwords;
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
repo_level = {"GITHUB_TOKEN", "SCCACHE_WEBDAV_RO_PASSWORD", "ARUGULA_GH_AUTHOR_TOKEN", "ARUGULA_GH_REVIEWER_TOKEN", "ARUGULA_GH_APP_PRIVATE_KEY"}
cache_write = {"SCCACHE_WEBDAV_RW_PASSWORD"}
cache = {"SCCACHE_WEBDAV_RO_PASSWORD"} | cache_write
from_source = {"release.yml": ["linux", "linux-arm64"]}
release = {"release.yml", "app-release.yml"}
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
        needs_env = set() if env else held - repo_level - (top & cache_write)
        if needs_env:
            fail.append(f"{name}: {job_name} reads {sorted(needs_env)} without an environment")
        if job_name in from_source.get(name, []) and held & cache:
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
