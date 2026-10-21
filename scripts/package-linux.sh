#!/bin/bash
# Builds Nori for Linux and packages AppImage and Debian packages via Tauri.
# Usage: scripts/package-linux.sh

set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> Packaging Nori for Linux"

# 1. Build CLI helper
echo "==> Step 1/3: Building nori CLI release binary..."
cargo build --release -p nori-cli

# 2. Stage helper for GUI bundle
echo "==> Step 2/3: Staging helper into crates/nori-gui/helper/..."
mkdir -p crates/nori-gui/helper
cp target/release/nori crates/nori-gui/helper/

# 3. Build GUI AppImage / deb
echo "==> Step 3/3: Building GUI package..."
cd crates/nori-gui
if command -v cargo-tauri >/dev/null 2>&1; then
    cargo tauri build --bundles appimage,deb --config '{"bundle":{"resources":{"helper/":"helper/"}}}'
else
    npx --yes @tauri-apps/cli@2.12 build --bundles appimage,deb --config '{"bundle":{"resources":{"helper/":"helper/"}}}'
fi

echo "==> Linux packages built in target/release/bundle/"
