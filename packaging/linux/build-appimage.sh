#!/bin/sh
# Builds dist/OpenDownloader-<version>-x86_64.AppImage from target/release/opendownloader.
# Usage: packaging/linux/build-appimage.sh [version]
set -eu
cd "$(dirname "$0")/../.."
VERSION="${1:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}"
ARCH="$(uname -m)"
BIN="${BIN:-target/release/opendownloader}"
[ -x "$BIN" ] || { echo "build first: cargo build --release -p odl-gui" >&2; exit 1; }

APPDIR="$(mktemp -d)/OpenDownloader.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"
install -m755 "$BIN" "$APPDIR/usr/bin/opendownloader"
install -m644 packaging/linux/io.github.opendownloader.desktop "$APPDIR/usr/share/applications/"
install -m644 packaging/linux/io.github.opendownloader.desktop "$APPDIR/"
install -m644 packaging/icons/256x256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/io.github.opendownloader.png"
install -m644 packaging/icons/256x256.png "$APPDIR/io.github.opendownloader.png"
ln -sf usr/bin/opendownloader "$APPDIR/AppRun"

TOOL="${APPIMAGETOOL:-$HOME/.cache/opendownloader-build/appimagetool-$ARCH.AppImage}"
if [ ! -x "$TOOL" ]; then
    mkdir -p "$(dirname "$TOOL")"
    curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$TOOL"
fi
mkdir -p dist
OUT="dist/OpenDownloader-$VERSION-$ARCH.AppImage"
# Extract-and-run avoids needing FUSE (CI, containers, WSL).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream "$APPDIR" "$OUT"
echo "built $OUT"
