#!/usr/bin/env bash
# Package existing Windows binaries. Native Release builds use build-windows.ps1.
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-debug}"
target="${2:-x86_64-pc-windows-gnu}"
[[ "$profile" == debug || "$profile" == release ]] || { echo "Expected debug or release" >&2; exit 1; }
[[ "$target" == x86_64-pc-windows-gnu || "$target" == x86_64-pc-windows-msvc ]] || { echo "Expected a Windows x86_64 target" >&2; exit 1; }
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
target_dir="${CARGO_TARGET_DIR:-target}"
out="${MIJIALAN_ARTIFACT_DIR:-$PWD/artifacts}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
label=""
[[ "$profile" == release ]] || label="-debug"
name="MijiaLAN-Rust-${version}-windows-x86_64${label}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
stage="$scratch/$name"
mkdir -p "$stage"
for binary in mijialan-rust mijia-rust; do
  cp "$target_dir/$target/$profile/$binary.exe" "$stage/"
done
cp LICENSE README.md PRIVACY.md THIRD_PARTY_NOTICES.md "$stage/"
cp -R licenses "$stage/licenses"
cp -R docs "$stage/docs"
# GNU test builds must remain self-contained if a native library needs these DLLs.
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
  # Inspect both binaries and any copied runtime's dependencies.
  for pass in 1 2 3; do
    imports="$(find "$stage" -maxdepth 1 \( -name '*.exe' -o -name '*.dll' \) -exec x86_64-w64-mingw32-objdump -p {} + | sed -n 's/.*DLL Name: //p')"
    for dll in libgcc_s_seh-1.dll libstdc++-6.dll libwinpthread-1.dll; do
      if [[ "$imports" == *"$dll"* && ! -f "$stage/$dll" ]]; then
        path="$(x86_64-w64-mingw32-gcc -print-file-name="$dll")"
        [[ -f "$path" ]] || { echo "Missing runtime $dll" >&2; exit 1; }
        cp "$path" "$stage/"
      fi
    done
  done
fi
(cd "$scratch" && zip -q -r "$name.zip" "$name")
mv "$scratch/$name.zip" "$out/$name.zip"
bash scripts/package-windows-msi.sh "$out/$name.zip"
# Keep NSIS opt-in while its generated test installer is under investigation.
if [[ "${MIJIALAN_BUILD_NSIS:-0}" == 1 ]]; then
  makensis -V2 "-DAPP_VERSION=$version" "-DSOURCE_DIR=$stage" "-DOUTPUT_FILE=$out/$name-Setup.exe" packaging/windows/installer.nsi
  (cd "$out" && sha256sum "$name-Setup.exe" >> "$name.sha256")
fi
printf '%s\n' "$out/$name.zip" "$out/$name.msi"
