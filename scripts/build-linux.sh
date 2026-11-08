#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-release}"
[[ "$profile" == debug || "$profile" == release ]] || { echo "Expected debug or release" >&2; exit 1; }
[[ "$(uname -s)" == Linux ]] || { echo "Run on Linux or in the build container" >&2; exit 1; }
args=(build --locked -p mijialan-desktop -p mijialan-cli)
[[ "$profile" != release ]] || args+=(--release)
cargo "${args[@]}"
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
case "$(uname -m)" in
  x86_64) arch=amd64; machine=x86_64 ;;
  aarch64) arch=arm64; machine=aarch64 ;;
  *) echo "Unsupported Linux architecture" >&2; exit 1 ;;
esac
target_dir="${CARGO_TARGET_DIR:-target}"
out="${MIJIALAN_ARTIFACT_DIR:-$PWD/artifacts}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
label=""
[[ "$profile" == release ]] || label="-debug"
name="mijialan-rust-${version}-linux-${machine}${label}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
stage="$scratch/$name"
mkdir -p "$stage"
cp "$target_dir/$profile/mijialan-rust" "$target_dir/$profile/mijia-rust" "$stage/"
cp LICENSE README.md PRIVACY.md THIRD_PARTY_NOTICES.md "$stage/"
cp -R licenses "$stage/licenses"
cp -R docs "$stage/docs"
tar -C "$scratch" -czf "$out/$name.tar.gz" "$name"
deb="$scratch/deb"
mkdir -p "$deb/DEBIAN" "$deb/usr/bin" "$deb/usr/share/applications" "$deb/usr/share/icons/hicolor/scalable/apps" "$deb/usr/share/doc/mijialan-rust"
cp "$stage/mijialan-rust" "$stage/mijia-rust" "$deb/usr/bin/"
cp packaging/linux/mijialan-rust.desktop "$deb/usr/share/applications/"
cp packaging/linux/mijialan-rust.svg "$deb/usr/share/icons/hicolor/scalable/apps/"
cp "$stage/LICENSE" "$stage/PRIVACY.md" "$stage/THIRD_PARTY_NOTICES.md" "$deb/usr/share/doc/mijialan-rust/"
cp -R licenses "$deb/usr/share/doc/mijialan-rust/licenses"
cp -R docs "$deb/usr/share/doc/mijialan-rust/docs"
cat > "$deb/DEBIAN/control" <<CONTROL
Package: mijialan-rust
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: iPocket
Depends: libc6 (>= 2.36), libgcc-s1, libstdc++6, libdbus-1-3, libasound2 | libasound2t64, libfontconfig1, libfreetype6, libx11-6, libx11-xcb1, libxcb1, libxcb-render0, libxcb-shape0, libxcb-xfixes0, libxcb-xkb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1
Recommends: mesa-vulkan-drivers, fonts-noto-cjk, gnome-keyring | keepassxc
Description: Mijia LAN controls and Apple Home bridge
 A Rust desktop client with local miIO/MIoT controls, QR import,
 encrypted credentials and a persistent HomeKit bridge.
CONTROL
dpkg-deb --root-owner-group --build "$deb" "$out/$name.deb"
(cd "$out" && sha256sum "$name.tar.gz" "$name.deb" > "$name.sha256")
printf '%s\n' "$out/$name.tar.gz" "$out/$name.deb"
