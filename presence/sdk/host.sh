#!/bin/sh
# `sdk/host.sh <static dir>`: the server SDKs as arugula.stream serves them,
# so each installs from one line without a public registry (the relay
# serves <static dir>, its priv/static):
#   npm/presence-server-<v>.tgz   npm install https://arugula.stream/npm/presence-server.tgz
#   npm/presence-react-<v>.tgz    npm install https://arugula.stream/npm/presence-react.tgz
#                                  (react/pack, once `vite build --config
#                                  vite.pack.config.ts` has filled its dist)
#   py/arugula_presence-<v>.tar.gz   pip install https://arugula.stream/py/arugula_presence-<v>.tar.gz
#   go/arugula.stream/presence/@v/…   go get arugula.stream/presence (Go's module
#                                  proxy protocol; the relay answers ?go-get=1)
#   gems/…                        gem install arugula_presence --source https://arugula.stream/gems
#                                  (only where `gem` is on PATH: the image's ruby stage)
# Elixir installs from git (a sparse dependency): Hex wants a signed registry.
set -eu
[ $# -eq 1 ] || { echo "usage: $0 <static dir>" >&2; exit 2; }
mkdir -p "$1"
out=$(realpath "$1")
cd "$(dirname "$0")"

# npm: the zero-dependency token package, packed as npm would publish it.
mkdir -p "$out/npm"
v=$(node -p 'require("./node/package.json").version')
(cd node && npm pack --silent --pack-destination "$out/npm" >/dev/null)
cp "$out/npm/arugula-presence-server-$v.tgz" "$out/npm/presence-server-$v.tgz"
mv "$out/npm/arugula-presence-server-$v.tgz" "$out/npm/presence-server.tgz"

# React: the bundled package (../react/pack).
if [ -f ../react/pack/dist/index.js ]; then
  rv=$(node -p 'require("../react/pack/package.json").version')
  (cd ../react/pack && npm pack --silent --pack-destination "$out/npm" >/dev/null)
  cp "$out/npm/arugula-presence-react-$rv.tgz" "$out/npm/presence-react-$rv.tgz"
  mv "$out/npm/arugula-presence-react-$rv.tgz" "$out/npm/presence-react.tgz"
fi

# Python: a source distribution; pip builds it with setuptools.
mkdir -p "$out/py"
pv=$(sed -n 's/^version = "\(.*\)"/\1/p' python/pyproject.toml)
python3 - "$out/py/arugula_presence-$pv.tar.gz" "$pv" <<'PY'
import os, sys, tarfile
dest, v = sys.argv[1], sys.argv[2]
root = f"arugula_presence-{v}"
with tarfile.open(dest, "w:gz") as t:
    for f in ["pyproject.toml", "README.md"]:
        t.add(os.path.join("python", f), f"{root}/{f}")
    for f in sorted(os.listdir("python/arugula_presence")):
        if f.endswith(".py"):
            t.add(os.path.join("python/arugula_presence", f), f"{root}/arugula_presence/{f}")
PY

# Go: the module proxy protocol (go.dev/ref/mod#goproxy-protocol) as files.
gv=v$(sed -n 's/.*Version = "\(.*\)".*/\1/p' go/presence.go | head -1)
[ "$gv" != v ] || gv=v$v
gdir="$out/go/arugula.stream/presence/@v"
mkdir -p "$gdir"
printf '%s\n' "$gv" > "$gdir/list"
printf '{"Version":"%s","Time":"2026-10-28T00:00:00Z"}\n' "$gv" > "$gdir/$gv.info"
cp "$gdir/$gv.info" "$out/go/arugula.stream/presence/@latest"
cp go/go.mod "$gdir/$gv.mod"
python3 - "$gdir/$gv.zip" "$gv" <<'PY'
import os, sys, zipfile
dest, v = sys.argv[1], sys.argv[2]
prefix = f"arugula.stream/presence@{v}/"
with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as z:
    for f in sorted(os.listdir("go")):
        if f.endswith((".go", ".mod", ".md")) or f == "LICENSE":
            z.write(os.path.join("go", f), prefix + f)
PY

# Ruby: a gem and a static index, where there's a ruby (`gem generate_index`
# is the rubygems-generate_index gem on Ruby 3.4+).
if (cd ruby && gem --version) >/dev/null 2>&1; then
  mkdir -p "$out/gems/gems"
  (
    cd ruby
    rv=$(ruby -e 'require "./lib/arugula_presence"; print ArugulaPresence::VERSION')
    gem build --silent arugula_presence.gemspec -o "$out/gems/gems/arugula_presence-$rv.gem" >/dev/null
    gem list -i rubygems-generate_index >/dev/null || gem install --silent rubygems-generate_index >/dev/null
    gem generate_index --directory "$out/gems" >/dev/null
  )
fi
echo "hosted the SDKs in $out"
