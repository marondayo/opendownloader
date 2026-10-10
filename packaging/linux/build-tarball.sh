#!/bin/sh
# Builds dist/opendownloader-<version>-linux-<arch>.tar.gz (binary + install script).
set -eu
cd "$(dirname "$0")/../.."
VERSION="${1:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}"
ARCH="$(uname -m)"
NAME="opendownloader-$VERSION-linux-$ARCH"
STAGE="$(mktemp -d)/$NAME"
mkdir -p "$STAGE/target/release" "$STAGE/packaging/linux" "$STAGE/packaging/icons" dist
install -m755 target/release/opendownloader "$STAGE/target/release/"
cp packaging/linux/install.sh packaging/linux/io.github.opendownloader.desktop packaging/linux/io.github.opendownloader.svg "$STAGE/packaging/linux/"
cp packaging/icons/*.png "$STAGE/packaging/icons/"
cp README.md LICENSE "$STAGE/"
printf '#!/bin/sh\nexec "$(dirname "$0")/packaging/linux/install.sh" "$(dirname "$0")/target/release/opendownloader"\n' > "$STAGE/install.sh"
chmod +x "$STAGE/install.sh"
tar -C "$(dirname "$STAGE")" -czf "dist/$NAME.tar.gz" "$NAME"
echo "built dist/$NAME.tar.gz"
