#!/usr/bin/env bash
# iPhone builds in WSL (stage 8, docs/11_DEV_ENV.md): `core` — the Rust core and Swift bindings
# with the xtool SDK (Linux has no xcrun, so SDKROOT and clang are given); `app` — the SwiftUI app
# by `xtool dev build` from a copy on ext4 (clang's module cache breaks on /mnt/d, spike-P-01);
# `package` — copies the built SirTone.app to the hub's dev package folder (SIRTONE_IOS_APP).
# Usage (from Windows): wsl -d Ubuntu-24.04 -- bash -lc "bash /mnt/d/SirVault/SirTone-mobile/scripts/wsl-ios.sh core|app|package"
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
case "${1:-}" in
  core)
    SDK="$HOME/.swiftpm/swift-sdks/darwin.artifactbundle/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
    export SDKROOT="$SDK"
    export CC_aarch64_apple_ios=clang CXX_aarch64_apple_ios=clang++ AR_aarch64_apple_ios=llvm-ar
    export CFLAGS_aarch64_apple_ios="-target arm64-apple-ios17.0 -isysroot $SDK"
    export CXXFLAGS_aarch64_apple_ios="-target arm64-apple-ios17.0 -isysroot $SDK"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/target-sirtone}" SIRTONE_IOS_SIM=0
    cd "$REPO" && bash scripts/build-ios-core.sh
    ;;
  app)
    DST="$HOME/build/mobile-ios"
    mkdir -p "$DST"
    rsync -a --delete --exclude xtool --exclude .build --exclude .bsp "$REPO/apps/mobile-ios/" "$DST/"
    cd "$DST" && xtool dev build
    ;;
  package)
    OUT="${SIRTONE_IOS_PACKAGE_DIR:-/mnt/d/SirVault/p4-app/src}"
    rm -rf "$OUT/SirTone.app.prev"
    [ -d "$OUT/SirTone.app" ] && mv "$OUT/SirTone.app" "$OUT/SirTone.app.prev"
    cp -r "$HOME/build/mobile-ios/xtool/SirTone.app" "$OUT/SirTone.app"
    echo "package: $OUT/SirTone.app"
    ;;
  *) echo "usage: wsl-ios.sh core|app|package" >&2; exit 2 ;;
esac
