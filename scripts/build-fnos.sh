#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
FN_PACK="${FN_PACK:-fnpack}"
if [ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ]; then
    cargo build --release --locked -p mijialan-daemon --target x86_64-unknown-linux-gnu
else
    cargo zigbuild --release --locked -p mijialan-daemon --target x86_64-unknown-linux-gnu.2.31
fi
BINARY="${CARGO_TARGET_DIR:-$ROOT/target}/x86_64-unknown-linux-gnu/release/mijialan-daemon"
mkdir -p packaging/fnos/app/bin packaging/fnos/app/licenses dist
cp "$BINARY" packaging/fnos/app/bin/
: "${CAMERA_RUNTIME_DIR:?Set CAMERA_RUNTIME_DIR to the Linux x86_64 camera runtime directory}"
for component in go2rtc ffmpeg; do
    test -f "$CAMERA_RUNTIME_DIR/bin/$component"
    file "$CAMERA_RUNTIME_DIR/bin/$component" | grep -q 'ELF 64-bit.*x86-64' || { echo "Wrong camera architecture: $component"; exit 1; }
    cp "$CAMERA_RUNTIME_DIR/bin/$component" packaging/fnos/app/bin/
    chmod 755 "packaging/fnos/app/bin/$component"
done
cp -R "$CAMERA_RUNTIME_DIR/licenses/." packaging/fnos/app/licenses/
cp LICENSE THIRD_PARTY_NOTICES.md packaging/fnos/app/licenses/
cp -R licenses/. packaging/fnos/app/licenses/
for script in packaging/fnos/cmd/*; do bash -n "$script"; done
cd dist
"$FN_PACK" build --directory "$ROOT/packaging/fnos"
mv mijialan.fpk MijiaLAN-fnOS-0.1.1-x86_64-preview.fpk
