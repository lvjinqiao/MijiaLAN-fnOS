#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-debug}"
signing_identity="${MIJIALAN_SIGNING_IDENTITY:--}"
[[ "$profile" == debug || "$profile" == release ]] || { echo "Expected debug or release" >&2; exit 1; }
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
build_number="${MIJIALAN_BUILD_NUMBER:-1}"
[[ "$build_number" =~ ^[1-9][0-9]*$ ]] || { echo "Expected a positive build number" >&2; exit 1; }
macos_target="${MIJIALAN_MACOS_TARGET:-}"
binary_root="${CARGO_TARGET_DIR:-target}"
cargo_args=(build --locked -p mijialan-desktop -p mijialan-cli)
if [[ -n "$macos_target" ]]; then
  case "$macos_target" in
    aarch64-apple-darwin|x86_64-apple-darwin) ;;
    *) echo "Expected a macOS Rust target" >&2; exit 1 ;;
  esac
  cargo_args+=(--target "$macos_target")
  binary_root="$binary_root/$macos_target"
fi
if [[ "$profile" == release ]]; then
  cargo_args+=(--release)
else
  profile=debug
fi
cargo "${cargo_args[@]}"
bundle="${MIJIALAN_APP_OUTPUT:-$PWD/build/MijiaLAN Rust.app}"
if [[ "${2:-}" == preview ]]; then
  bundle="$PWD/build/MijiaLAN Rust Preview.app"
fi
[[ "$bundle" == /*.app ]] || { echo "App output must be an absolute .app path" >&2; exit 1; }
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binary_root/$profile/mijialan-rust" "$bundle/Contents/MacOS/mijialan-rust"
cp "$binary_root/$profile/mijia-rust" "$bundle/Contents/MacOS/mijia-rust"
cp LICENSE THIRD_PARTY_NOTICES.md PRIVACY.md "$bundle/Contents/Resources/"
ditto licenses "$bundle/Contents/Resources/Licenses"
if [[ -f resources/macos/AppIcon.icns ]]; then
  cp resources/macos/AppIcon.icns "$bundle/Contents/Resources/AppIcon.icns"
fi
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>mijialan-rust</string>
<key>CFBundleIdentifier</key><string>xyz.ipocket.MijiaLAN.Rust</string>
<key>CFBundleName</key><string>MijiaLAN Rust</string>
<key>CFBundleDisplayName</key><string>MijiaLAN Rust</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSUIElement</key><true/>
<key>NSLocalNetworkUsageDescription</key><string>发现和控制同一局域网的米家设备，并提供 HomeKit 家庭桥接。</string>
<key>NSBonjourServices</key><array><string>_hap._tcp</string></array>
</dict></plist>
PLIST
plutil -replace CFBundleShortVersionString -string "$version" "$bundle/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$build_number" "$bundle/Contents/Info.plist"
if [[ -n "${MIJIALAN_APP_NAME:-}" ]]; then
  plutil -replace CFBundleName -string "$MIJIALAN_APP_NAME" "$bundle/Contents/Info.plist"
  plutil -replace CFBundleDisplayName -string "$MIJIALAN_APP_NAME" "$bundle/Contents/Info.plist"
fi
if [[ -f "$bundle/Contents/Resources/AppIcon.icns" ]]; then
  plutil -insert CFBundleIconFile -string AppIcon "$bundle/Contents/Info.plist"
fi
if [[ "${2:-}" == preview ]]; then
  plutil -replace CFBundleIdentifier -string xyz.ipocket.MijiaLAN.Rust.Preview "$bundle/Contents/Info.plist"
  plutil -replace CFBundleName -string "MijiaLAN Rust Preview" "$bundle/Contents/Info.plist"
  plutil -replace CFBundleDisplayName -string "MijiaLAN Rust Preview" "$bundle/Contents/Info.plist"
fi
plutil -lint "$bundle/Contents/Info.plist"
if [[ "$signing_identity" == - ]]; then
  codesign --force --sign - --timestamp=none "$bundle/Contents/MacOS/mijia-rust"
  codesign --force --sign - --timestamp=none "$bundle"
else
  codesign --force --sign "$signing_identity" --options runtime --timestamp "$bundle/Contents/MacOS/mijia-rust"
  codesign --force --sign "$signing_identity" --options runtime --timestamp "$bundle"
fi
codesign --verify --strict "$bundle"
echo "$bundle"
