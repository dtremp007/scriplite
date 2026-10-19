#!/bin/sh
# Download the pinned Pdfium build for one platform and verify its checksum.
# Prints the path of the extracted library.
set -eu

PDFIUM_VERSION=7881
PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

usage() {
    cat <<'EOF'
Usage: fetch_pdfium.sh [PLATFORM]

PLATFORM is one of linux-x64, linux-arm64, mac-arm64, mac-x64.
It defaults to the platform of the current machine.
EOF
}

host_platform() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo linux-x64 ;;
        Linux-aarch64 | Linux-arm64) echo linux-arm64 ;;
        Darwin-arm64) echo mac-arm64 ;;
        Darwin-x86_64) echo mac-x64 ;;
        *)
            printf 'error: unsupported host %s-%s\n' "$(uname -s)" "$(uname -m)" >&2
            exit 3
            ;;
    esac
}

case "${1:-}" in
    -h | --help)
        usage
        exit 0
        ;;
    "") PLATFORM=$(host_platform) ;;
    *) PLATFORM=$1 ;;
esac

case "$PLATFORM" in
    linux-x64)
        SHA256=1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d
        LIBRARY=libpdfium.so
        ;;
    linux-arm64)
        SHA256=ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674
        LIBRARY=libpdfium.so
        ;;
    mac-arm64)
        SHA256=52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40
        LIBRARY=libpdfium.dylib
        ;;
    mac-x64)
        SHA256=6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b
        LIBRARY=libpdfium.dylib
        ;;
    *)
        printf 'error: unknown platform: %s\n' "$PLATFORM" >&2
        usage >&2
        exit 2
        ;;
esac

VENDOR_DIR="$PROJECT_DIR/vendor/pdfium/$PLATFORM"
URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_VERSION}/pdfium-${PLATFORM}.tgz"

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

if [ ! -f "$VENDOR_DIR/lib/$LIBRARY" ]; then
    mkdir -p "$VENDOR_DIR"
    ARCHIVE="$VENDOR_DIR/pdfium.tgz"
    trap 'rm -f "$ARCHIVE"' EXIT HUP INT TERM
    curl --fail --silent --show-error --location "$URL" --output "$ARCHIVE"
    ACTUAL=$(sha256_of "$ARCHIVE")
    if [ "$ACTUAL" != "$SHA256" ]; then
        printf 'error: checksum mismatch for %s\n  expected %s\n  actual   %s\n' \
            "$URL" "$SHA256" "$ACTUAL" >&2
        exit 3
    fi
    tar -xzf "$ARCHIVE" -C "$VENDOR_DIR"
fi

printf '%s\n' "$VENDOR_DIR/lib/$LIBRARY"
