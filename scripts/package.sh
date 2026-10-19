#!/bin/sh
# Build a self-contained release archive for one platform.
# Linux builds use cargo-zigbuild and target glibc 2.17 so the binary runs on
# any mainstream distribution; Pdfium's own Linux builds have the same floor.
set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
GLIBC_FLOOR=2.17

usage() {
    cat <<'EOF'
Usage: package.sh [PLATFORM]

PLATFORM is one of linux-x64, linux-arm64, mac-arm64, mac-x64.
It defaults to the platform of the current machine.
Linux platforms require cargo-zigbuild and zig.
EOF
}

case "${1:-}" in
    -h | --help)
        usage
        exit 0
        ;;
    "") PLATFORM=$(uname -s)-$(uname -m) ;;
    *) PLATFORM=$1 ;;
esac

case "$PLATFORM" in
    linux-x64 | Linux-x86_64)
        PLATFORM=linux-x64
        TARGET=x86_64-unknown-linux-gnu
        ;;
    linux-arm64 | Linux-aarch64 | Linux-arm64)
        PLATFORM=linux-arm64
        TARGET=aarch64-unknown-linux-gnu
        ;;
    mac-arm64 | Darwin-arm64)
        PLATFORM=mac-arm64
        TARGET=aarch64-apple-darwin
        ;;
    mac-x64 | Darwin-x86_64)
        PLATFORM=mac-x64
        TARGET=x86_64-apple-darwin
        ;;
    *)
        printf 'error: unsupported platform: %s\n' "$PLATFORM" >&2
        usage >&2
        exit 2
        ;;
esac

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$PROJECT_DIR/Cargo.toml" | head -n 1)
NAME="scriplite-$VERSION-$PLATFORM"
DIST_DIR="$PROJECT_DIR/dist/$NAME"
PDFIUM_LIB=$("$PROJECT_DIR/scripts/fetch_pdfium.sh" "$PLATFORM")
PDFIUM_DIR=$(dirname -- "$(dirname -- "$PDFIUM_LIB")")

case "$PLATFORM" in
    linux-*)
        cargo zigbuild --manifest-path "$PROJECT_DIR/Cargo.toml" --locked --release \
            --target "$TARGET.$GLIBC_FLOOR"
        ;;
    *)
        cargo build --manifest-path "$PROJECT_DIR/Cargo.toml" --locked --release \
            --target "$TARGET"
        ;;
esac

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/licenses/pdfium"
cp "$PROJECT_DIR/target/$TARGET/release/scriplite" "$DIST_DIR/"
cp "$PDFIUM_LIB" "$DIST_DIR/"
cp "$PROJECT_DIR/scripts/install.sh" "$DIST_DIR/"
cp "$PROJECT_DIR/README.md" "$DIST_DIR/"
cp "$PDFIUM_DIR/LICENSE" "$DIST_DIR/licenses/pdfium/"
cp "$PDFIUM_DIR"/licenses/* "$DIST_DIR/licenses/pdfium/"
chmod 755 "$DIST_DIR/scriplite" "$DIST_DIR/install.sh"

ARCHIVE="$PROJECT_DIR/dist/$NAME.tar.gz"
tar -czf "$ARCHIVE" -C "$PROJECT_DIR/dist" "$NAME"
(
    cd "$PROJECT_DIR/dist"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256"
    else
        shasum -a 256 "$NAME.tar.gz" >"$NAME.tar.gz.sha256"
    fi
)

printf '%s\n' "$ARCHIVE"
