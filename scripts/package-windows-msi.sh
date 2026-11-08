#!/usr/bin/env bash
# Build a standard MSI with no custom actions from an existing portable ZIP.
set -euo pipefail
cd "$(dirname "$0")/.."
input="${1:?Pass the Windows x86_64 portable ZIP}"
input="$(realpath "$input")"
[[ "$input" == *-windows-x86_64*.zip ]] || { echo "Expected a Windows x86_64 ZIP" >&2; exit 1; }
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
python3 - "$input" "$scratch" <<'PY'
from pathlib import Path
from zipfile import ZipFile
import sys

with ZipFile(sys.argv[1]) as archive:
    for name in archive.namelist():
        path = Path(name)
        if path.is_absolute() or ".." in path.parts:
            raise SystemExit("Unsafe ZIP path")
    archive.extractall(Path(sys.argv[2]) / "payload")
PY
roots=("$scratch/payload/"*)
[[ "${#roots[@]}" == 1 && -d "${roots[0]}" ]] || { echo "Expected a single application folder" >&2; exit 1; }
stage="${roots[0]}"
[[ -f "$stage/mijialan-rust.exe" && -f "$stage/mijia-rust.exe" ]] || { echo "Missing application binaries" >&2; exit 1; }
python3 packaging/windows/generate_msi.py "$stage" "$version" "$scratch/application.wxs"
output="${input%.zip}.msi"
wixl -a x64 -o "$scratch/application.msi" "$scratch/application.wxs"
mv "$scratch/application.msi" "$output"
directory="$(dirname "$input")"
name="$(basename "$input" .zip)"
(cd "$directory" && sha256sum "$name.zip" "$name.msi" > "$name.sha256")
printf '%s\n' "$output"
