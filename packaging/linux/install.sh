#!/bin/sh
# Installs OpenDownloader for the current user (~/.local).
# Usage: packaging/linux/install.sh [path/to/opendownloader]
set -e
BIN="${1:-target/release/opendownloader}"
HERE="$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$BIN" "$PREFIX/bin/opendownloader"
install -Dm644 "$HERE/io.github.opendownloader.desktop" "$PREFIX/share/applications/io.github.opendownloader.desktop"
install -Dm644 "$HERE/io.github.opendownloader.svg" "$PREFIX/share/icons/hicolor/scalable/apps/io.github.opendownloader.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
echo "Installed to $PREFIX/bin/opendownloader"
