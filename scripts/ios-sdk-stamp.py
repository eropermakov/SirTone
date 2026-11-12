#!/usr/bin/env python3
"""Stamps a built SirTone.app with the SDK it was really built against (wsl-ios.sh `app`).

xtool builds on Linux link with `-target arm64-apple-ios17.0` and no `-platform_version`, so the
Mach-O's LC_BUILD_VERSION says "SDK 17.0". iOS reads that as "built for iOS 17" and runs the app
in compatibility mode: no Liquid Glass, the old tab bar (D-169 never shows). Xcode writes the real
SDK there and the DT* keys into Info.plist; this does the same, before any signature (the hub
signs the package afterwards, D-154).

Usage: ios-sdk-stamp.py <SirTone.app> <iPhoneOS.sdk> [version]
With a version (release-ios.ps1), it also goes into CFBundleShortVersionString/CFBundleVersion,
so the phone's Settings show which build is installed.
"""
import json
import plistlib
import struct
import sys
from pathlib import Path

LC_BUILD_VERSION = 0x32
MH_MAGIC_64 = 0xFEEDFACF
PLATFORM_IOS = 2


def packed(v: str) -> int:
    parts = [int(x) for x in v.split(".")] + [0, 0]
    return (parts[0] << 16) | (parts[1] << 8) | parts[2]


def stamp_binary(path: Path, sdk: int) -> None:
    b = bytearray(path.read_bytes())
    magic, _cpu, _sub, _ft, ncmds, _size, _flags, _res = struct.unpack_from("<8I", b, 0)
    if magic != MH_MAGIC_64:
        sys.exit(f"not a thin 64-bit Mach-O: {path}")
    off, found = 32, 0
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", b, off)
        if cmd == LC_BUILD_VERSION:
            platform, _minos, old = struct.unpack_from("<III", b, off + 8)
            if platform != PLATFORM_IOS:
                sys.exit(f"unexpected platform {platform}")
            struct.pack_into("<I", b, off + 16, sdk)
            found += 1
        off += size
    if found != 1:
        sys.exit(f"expected one LC_BUILD_VERSION, found {found}")
    path.write_bytes(bytes(b))


def main() -> None:
    app, sdk_dir = Path(sys.argv[1]), Path(sys.argv[2])
    release = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else None
    settings = json.loads((sdk_dir / "SDKSettings.json").read_text())
    version = settings["Version"]  # e.g. "27.1"
    name = settings.get("CanonicalName", f"iphoneos{version}")
    info_path = app / "Info.plist"
    with info_path.open("rb") as f:
        info = plistlib.load(f)
    stamp_binary(app / info["CFBundleExecutable"], packed(version))
    info.update(
        {
            "DTPlatformName": "iphoneos",
            "DTPlatformVersion": version,
            "DTSDKName": name,
        }
    )
    if release:
        info["CFBundleShortVersionString"] = release
        info["CFBundleVersion"] = release
    with info_path.open("wb") as f:
        plistlib.dump(info, f)
    print(f"stamped: SDK {version} ({name}), version {release or info.get('CFBundleShortVersionString')}")


if __name__ == "__main__":
    main()
