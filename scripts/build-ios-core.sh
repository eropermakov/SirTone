#!/usr/bin/env bash
# Rust core for the iPhone app (D-137/D-138, M1.4): static libraries for the device and the
# simulator, packed into apps/mobile-ios/Frameworks/SirToneCoreFFI.xcframework, plus the UniFFI
# Swift bindings in apps/mobile-ios/Sources/SirToneCore/. Runs on macOS (CI) or Linux/WSL with
# the iOS SDK from `xtool setup` (then only the device slice; set SIRTONE_IOS_SIM=0).
set -euo pipefail
cd "$(dirname "$0")/.."

CRATE=sirtone-mobile-ffi
LIB=libsirtone_mobile_ffi.a
OUT=apps/mobile-ios/Frameworks/SirToneCoreFFI.xcframework
SWIFT_OUT=apps/mobile-ios/Sources/SirToneCore
TD="${CARGO_TARGET_DIR:-target}"
GEN="$TD/mobile-gen"
TARGETS=(aarch64-apple-ios)
if [[ "${SIRTONE_IOS_SIM:-1}" == 1 ]]; then TARGETS+=(aarch64-apple-ios-sim); fi
export IPHONEOS_DEPLOYMENT_TARGET=17.0

rustup target add "${TARGETS[@]}" >/dev/null
for t in "${TARGETS[@]}"; do
  # Only the static library: the app links it; a dylib would need the Apple linker (absent on Linux).
  cargo rustc --release --locked -p "$CRATE" --lib --target "$t" --crate-type staticlib
done

# Bindings from a host build (library mode reads the UniFFI metadata from the binary).
# Unstripped: the release profile strips symbols, and with them the UniFFI metadata.
export CARGO_PROFILE_RELEASE_STRIP=none
cargo build --release --locked -p "$CRATE" --lib
HOST_LIB=$( (ls "$TD"/release/libsirtone_mobile_ffi.{dylib,so} 2>/dev/null || true) | head -1)
rm -rf "$GEN" && mkdir -p "$GEN"
cargo run --release --locked -q -p "$CRATE" --features bindgen --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language swift --out-dir "$GEN" --no-format

mkdir -p "$GEN/headers"
cp "$GEN"/*.h "$GEN/headers/"
cp "$GEN"/*.modulemap "$GEN/headers/module.modulemap"
rm -rf "$SWIFT_OUT" && mkdir -p "$SWIFT_OUT"
cp "$GEN"/*.swift "$SWIFT_OUT/"

rm -rf "$OUT"
if command -v xcodebuild >/dev/null; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(-library "$TD/$t/release/$LIB" -headers "$GEN/headers"); done
  xcodebuild -create-xcframework "${args[@]}" -output "$OUT"
else
  # Linux/WSL: the same layout xcodebuild writes, device slice only.
  mkdir -p "$OUT/ios-arm64/Headers"
  cp "$TD/aarch64-apple-ios/release/$LIB" "$OUT/ios-arm64/"
  cp "$GEN/headers/"* "$OUT/ios-arm64/Headers/"
  cat > "$OUT/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AvailableLibraries</key>
  <array>
    <dict>
      <key>LibraryIdentifier</key><string>ios-arm64</string>
      <key>LibraryPath</key><string>$LIB</string>
      <key>HeadersPath</key><string>Headers</string>
      <key>SupportedArchitectures</key><array><string>arm64</string></array>
      <key>SupportedPlatform</key><string>ios</string>
    </dict>
  </array>
  <key>CFBundlePackageType</key><string>XFWK</string>
  <key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict>
</plist>
PLIST
fi
echo "core: $OUT, bindings: $SWIFT_OUT"
