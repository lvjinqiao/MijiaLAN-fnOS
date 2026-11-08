#!/usr/bin/env bash
# Reuse the existing protocol tests on a Windows VM without installing Rust.
set -euo pipefail
cd "$(dirname "$0")/.."
target=x86_64-pc-windows-gnu
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
out="${MIJIALAN_ARTIFACT_DIR:-$PWD/artifacts}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
name="MijiaLAN-Rust-${version}-windows-x86_64-checks"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
stage="$scratch/$name"
mkdir -p "$stage/checks"
cargo test --locked --target "$target" -p mijialan-core -p mijialan-homekit --no-run --message-format=json > "$scratch/compiler.json"
python3 - "$scratch/compiler.json" "$stage/checks" <<'PY'
import json
from pathlib import Path
import shutil
import sys

executables = set()
for line in Path(sys.argv[1]).read_text().splitlines():
    item = json.loads(line)
    if item.get("reason") == "compiler-artifact" and item.get("executable") and item.get("profile", {}).get("test"):
        executables.add(item["executable"])
if not executables:
    raise SystemExit("No Windows test executables were produced")
for executable in sorted(executables):
    shutil.copy2(executable, Path(sys.argv[2]) / Path(executable).name)
print(f"Packaged {len(executables)} backend test executables")
PY
cp packaging/windows/Run-checks.cmd "$stage/"
cp LICENSE THIRD_PARTY_NOTICES.md "$stage/"
cp -R licenses "$stage/licenses"
cat > "$stage/README.txt" <<'README'
Run Run-checks.cmd on Windows 10/11 x64.
These are the project's existing core and HomeKit tests, including local UDP/TCP
protocol peers, pairing, encryption and event notifications. No real device
credentials are used or imported. Some tests advertise a temporary demo bridge.
This does not validate GPU rendering, the tray, QR login or physical devices.
README
(cd "$scratch" && zip -q -r "$name.zip" "$name")
mv "$scratch/$name.zip" "$out/$name.zip"
(cd "$out" && sha256sum "$name.zip" > "$name.sha256")
