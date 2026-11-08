#!/usr/bin/env bash
# Build a signed, notarized Apple Silicon experience package. Does not publish it.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
source scripts/lib/notarization.sh
[[ "$(uname -s)" == Darwin ]] || { echo "Run on a Mac" >&2; exit 1; }
if [[ "$(uname -m)" != arm64 && "${MIJIALAN_MACOS_TARGET:-}" != aarch64-apple-darwin ]]; then
  echo "Set MIJIALAN_MACOS_TARGET=aarch64-apple-darwin to build the Apple Silicon package on Intel" >&2
  exit 1
fi
[[ "${MIJIALAN_MACOS_TARGET:-aarch64-apple-darwin}" == aarch64-apple-darwin ]] || { echo "This package requires the Apple Silicon target" >&2; exit 1; }
: "${MIJIALAN_SIGNING_IDENTITY:?Set a Developer ID Application signing identity}"
[[ "$MIJIALAN_SIGNING_IDENTITY" != - ]] || { echo "Distribution requires Developer ID signing" >&2; exit 1; }
if [[ -n "${APPLE_API_KEY_PATH:-}" ]]; then
  : "${APPLE_API_KEY_ID:?Set the notarization team API key ID}"
  : "${APPLE_API_ISSUER_ID:?Set the notarization team API issuer ID}"
  [[ -r "$APPLE_API_KEY_PATH" ]] || { echo "Notarization key file is not readable" >&2; exit 1; }
  notary_auth=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
else
  : "${MIJIALAN_NOTARY_PROFILE:?Set an existing notarytool Keychain profile or APPLE_API_KEY_PATH}"
  notary_auth=(--keychain-profile "$MIJIALAN_NOTARY_PROFILE")
fi
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
build_number="${MIJIALAN_BUILD_NUMBER:-3}"
[[ "$build_number" =~ ^[1-9][0-9]*$ ]] || { echo "Expected a positive build number" >&2; exit 1; }
release="$root/build/releases/v$version-rs.$build_number"
staging="$release/staging"
dist="$release/dist"
logs="$release/logs"
app="$staging/MijiaLAN.app"
name="MijiaLAN-rs-$version-b$build_number-macos-arm64"
mkdir -p "$staging" "$dist" "$logs"

case "${1:-}" in
  "")
    cargo fmt --all --check 2>&1 | tee "$logs/format.log"
    test_args=(test --locked --workspace -j "${CARGO_BUILD_JOBS:-4}")
    case "${MIJIALAN_TEST_PROFILE:-release}" in
      release) test_args+=(--release) ;;
      debug) ;;
      *) echo "Expected release or debug test profile" >&2; exit 1 ;;
    esac
    cargo "${test_args[@]}" 2>&1 | tee "$logs/tests.log"
    MIJIALAN_BUILD_NUMBER="$build_number" MIJIALAN_APP_OUTPUT="$app" MIJIALAN_APP_NAME=MijiaLAN \
      bash scripts/build-macos.sh release 2>&1 | tee "$logs/bundle.log"
    cargo metadata --locked --format-version 1 --filter-platform aarch64-apple-darwin > "$release/cargo-metadata.json"
    python3 scripts/collect-cargo-licenses.py "$release/cargo-metadata.json" "$app/Contents/Resources/Licenses/Cargo"
    ;;
  --package-existing) echo "Packaging the existing signed app: $app" ;;
  *) echo "Usage: $0 [--package-existing]" >&2; exit 1 ;;
esac

# A private runner umask must not make the distributed app owner-only.
# find does not follow symlinks, so linked files outside the bundle stay intact.
find "$app" -type d -exec chmod 755 {} +
find "$app" -type f -exec chmod u=rwX,go=rX {} +
# Notices are part of the sealed resources; sign again after adding them.
codesign --force --sign "$MIJIALAN_SIGNING_IDENTITY" --options runtime --timestamp "$app"

[[ "$(plutil -extract CFBundleIdentifier raw "$app/Contents/Info.plist")" == xyz.ipocket.MijiaLAN.Rust ]]
[[ "$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")" == "$version" ]]
[[ "$(plutil -extract CFBundleVersion raw "$app/Contents/Info.plist")" == "$build_number" ]]
for binary in mijialan-rust mijia-rust; do
  lipo "$app/Contents/MacOS/$binary" -verify_arch arm64
  codesign --verify --strict "$app/Contents/MacOS/$binary"
done
codesign --verify --deep --strict "$app"
codesign --display --verbose=4 "$app" 2> "$logs/signature.txt"
grep -q '^Authority=Developer ID Application:' "$logs/signature.txt"
grep -q 'flags=.*runtime' "$logs/signature.txt"
grep -q '^Timestamp=' "$logs/signature.txt"

ditto -c -k --sequesterRsrc --keepParent "$app" "$release/notarization.zip"
xcrun notarytool submit "$release/notarization.zip" "${notary_auth[@]}" \
  --wait --timeout 20m --output-format json > "$logs/notarization-app.json"
[[ "$(plutil -extract status raw "$logs/notarization-app.json")" == Accepted ]]
retry_notarization_ticket xcrun stapler staple "$app"
retry_notarization_ticket xcrun stapler validate "$app"
spctl --assess --type execute --verbose=4 "$app" 2> "$logs/gatekeeper-app.txt"
ditto -c -k --sequesterRsrc --keepParent "$app" "$dist/$name.zip"

image_root="$release/dmg-root"
mkdir -p "$image_root"
ditto "$app" "$image_root/MijiaLAN.app"
ln -sfn /Applications "$image_root/Applications"
cp LICENSE THIRD_PARTY_NOTICES.md PRIVACY.md "$image_root/"
cat > "$image_root/体验说明.txt" <<'NOTE'
MijiaLAN · Rust 版 · Apple Silicon

将 MijiaLAN.app 拖入 Applications，或在磁盘映像中直接打开。
本版本使用独立于 Swift 版的数据目录，保留此前 Rust 版导入的设备和配对资料。
请避免同时运行两个使用同一 Rust 数据目录的桥接实例。
首次使用 Developer ID 签名版本时，macOS 可能要求重新允许读取已有钥匙串凭据。
NOTE
chmod 755 "$image_root"
chmod 644 "$image_root/LICENSE" "$image_root/THIRD_PARTY_NOTICES.md" "$image_root/PRIVACY.md" "$image_root/体验说明.txt"
hdiutil create -volname "MijiaLAN rs $version" -srcfolder "$image_root" -format UDZO -ov "$dist/$name.dmg"
codesign --force --sign "$MIJIALAN_SIGNING_IDENTITY" --timestamp "$dist/$name.dmg"
codesign --verify --strict "$dist/$name.dmg"
xcrun notarytool submit "$dist/$name.dmg" "${notary_auth[@]}" \
  --wait --timeout 20m --output-format json > "$logs/notarization-dmg.json"
[[ "$(plutil -extract status raw "$logs/notarization-dmg.json")" == Accepted ]]
retry_notarization_ticket xcrun stapler staple "$dist/$name.dmg"
retry_notarization_ticket xcrun stapler validate "$dist/$name.dmg"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dist/$name.dmg" 2> "$logs/gatekeeper-dmg.txt"
(cd "$dist" && shasum -a 256 "$name.dmg" "$name.zip" > SHA256SUMS.txt)
echo "Signed and notarized packages: $dist"
