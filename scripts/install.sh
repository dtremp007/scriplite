#!/bin/sh
# Install scriplite from an extracted release archive (this script ships inside it).
set -eu

SOURCE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$(id -u)" = 0 ]; then
    PREFIX=/usr/local
else
    PREFIX="${HOME}/.local"
fi

usage() {
    cat <<'EOF'
Usage: install.sh [--prefix DIR]

Install scriplite and its bundled Pdfium library.

Options:
  --prefix DIR  Installation prefix (default: /usr/local as root, ~/.local otherwise)
  -h, --help    Show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            if [ "$#" -lt 2 ]; then
                printf '%s\n' "error: --prefix requires a directory" >&2
                exit 2
            fi
            PREFIX=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            printf 'error: unknown option: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

LIBRARY=
for candidate in libpdfium.so libpdfium.dylib; do
    if [ -f "$SOURCE_DIR/$candidate" ]; then
        LIBRARY=$candidate
    fi
done
if [ ! -f "$SOURCE_DIR/scriplite" ] || [ -z "$LIBRARY" ]; then
    printf 'error: run this from an extracted scriplite release archive (%s)\n' \
        "$SOURCE_DIR" >&2
    printf '%s\n' "       from a source checkout, run scripts/package.sh first" >&2
    exit 3
fi

TOOL_DIR="$PREFIX/libexec/scriplite"
BIN_DIR="$PREFIX/bin"
LAUNCHER="$BIN_DIR/scriplite"
LAUNCHER_TMP="$BIN_DIR/.scriplite.tmp.$$"

mkdir -p "$TOOL_DIR" "$BIN_DIR"
TOOL_DIR=$(CDPATH= cd -- "$TOOL_DIR" && pwd)
trap 'rm -f "$LAUNCHER_TMP"' EXIT HUP INT TERM

install -m 755 "$SOURCE_DIR/scriplite" "$TOOL_DIR/scriplite"
install -m 644 "$SOURCE_DIR/$LIBRARY" "$TOOL_DIR/$LIBRARY"
if [ -d "$SOURCE_DIR/licenses" ]; then
    rm -rf "$TOOL_DIR/licenses"
    cp -R "$SOURCE_DIR/licenses" "$TOOL_DIR/licenses"
fi

cat >"$LAUNCHER_TMP" <<EOF
#!/bin/sh
PDFIUM_LIB_PATH="$TOOL_DIR/$LIBRARY"
export PDFIUM_LIB_PATH
exec "$TOOL_DIR/scriplite" "\$@"
EOF
chmod 755 "$LAUNCHER_TMP"
mv -f "$LAUNCHER_TMP" "$LAUNCHER"
trap - EXIT HUP INT TERM

"$LAUNCHER" --version
printf 'Installed launcher: %s\n' "$LAUNCHER"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        printf '\nAdd this line to your shell profile (~/.bashrc or ~/.zshrc):\n'
        printf 'export PATH="%s:$PATH"\n' "$BIN_DIR"
        ;;
esac
