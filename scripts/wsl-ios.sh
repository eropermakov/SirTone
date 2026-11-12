#!/usr/bin/env bash
# iPhone builds in WSL (stage 8, docs/11_DEV_ENV.md): `core` — the Rust core and Swift bindings
# with the xtool SDK (Linux has no xcrun, so SDKROOT and clang are given); `app` — the SwiftUI app
# by `xtool dev build` from a copy on ext4 (clang's module cache breaks on /mnt/d, spike-P-01);
# `package` — copies the built SirTone.app to the hub's dev package folder (SIRTONE_IOS_APP);
# `ipa` — zips the built app as Payload/SirTone.app into $SIRTONE_IOS_IPA (scripts/release-ios.ps1).
# Before `app`: the visualiser page (F22) is built on Windows — `pnpm --dir apps/desktop build:mobile-viz`;
# without it the app builds without the visualiser.
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
    # Release by default (the packages people install); SIRTONE_IOS_CONFIG=debug for quick tries.
    cd "$DST" && xtool dev build -c "${SIRTONE_IOS_CONFIG:-release}"
    # The real SDK into the binary, as Xcode does: else iOS 26+ runs it as an iOS 17 app (D-169).
    SDK="$HOME/.swiftpm/swift-sdks/darwin.artifactbundle/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
    python3 "$REPO/scripts/ios-sdk-stamp.py" "$DST/xtool/SirTone.app" "$SDK" "${SIRTONE_IOS_VERSION:-}"
    ;;
  package)
    OUT="${SIRTONE_IOS_PACKAGE_DIR:-/mnt/d/SirVault/p4-app/src}"
    rm -rf "$OUT/SirTone.app.prev"
    [ -d "$OUT/SirTone.app" ] && mv "$OUT/SirTone.app" "$OUT/SirTone.app.prev"
    cp -r "$HOME/build/mobile-ios/xtool/SirTone.app" "$OUT/SirTone.app"
    echo "package: $OUT/SirTone.app"
    ;;
  ipa)
    OUT="${SIRTONE_IOS_IPA:?where to write the .ipa}"
    T="$(mktemp -d)"
    mkdir "$T/Payload"
    cp -r "$HOME/build/mobile-ios/xtool/SirTone.app" "$T/Payload/SirTone.app"
    # Published as it is (D-154): never with a signature or a profile of anybody's.
    if [ -e "$T/Payload/SirTone.app/embedded.mobileprovision" ] || [ -d "$T/Payload/SirTone.app/_CodeSignature" ]; then
      echo "refusing: the app is signed" >&2; rm -rf "$T"; exit 1
    fi
    rm -f "$OUT"
    (cd "$T" && zip -qr -X "$OUT" Payload)
    rm -rf "$T"
    echo "ipa: $OUT"
    ;;
  *) echo "usage: wsl-ios.sh core|app|package|ipa" >&2; exit 2 ;;
esac
