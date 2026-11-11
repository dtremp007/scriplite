# scriplite

`scriplite` preserves a screenplay PDF and places pale-yellow rectangles
under one speaking character's cues, parentheticals, dialogue, and continuation
markers. It creates one same-pagination PDF per selected speaker.

Version 1 supports standard screenplay PDFs with an embedded text layer. OCR,
screenplay reflow, formatting repair, rotated pages, and signed or encrypted PDFs
are intentionally unsupported.

## Supported platforms

Release archives are built for Linux x86_64 and arm64 (glibc 2.17 or newer:
CentOS/RHEL 7+, Debian 9+, Ubuntu 18.04+, Amazon Linux 2+) and for macOS
arm64 and x86_64. Musl-based distributions such as Alpine are not supported.
Each archive bundles PDFium build 7881, which is used for positioned text
extraction, together with its license files.

## Install on a VM from a release

Download the archive for the VM's architecture from the
[releases page](https://github.com/dtremp007/scriplite/releases)
(`uname -m`: `x86_64` is `linux-x64`, `aarch64` is `linux-arm64`), then
extract and install:

```sh
curl -fLO https://github.com/dtremp007/scriplite/releases/download/v0.1.0/scriplite-0.1.0-linux-x64.tar.gz
tar -xzf scriplite-0.1.0-linux-x64.tar.gz
sudo ./scriplite-0.1.0-linux-x64/install.sh
scriplite --version
```

As root the installer uses `/usr/local`; otherwise it installs under
`~/.local` and prints the `PATH` line to add if needed. Use `--prefix DIR` to
install somewhere else. The binary and PDFium go in `PREFIX/libexec/scriplite/`
and the launcher in `PREFIX/bin/scriplite`.

## Run with Docker

```sh
docker build -t scriplite .
docker run --rm -v "$PWD:/work" scriplite inspect "Dee Clock - 2026-07-25.pdf"
```

The container runs as UID 1000; pass `--user "$(id -u):$(id -g)"` if the
mounted directory belongs to someone else and you are generating PDFs.

## Build from source

Requires Rust 1.88 or newer. Fetch the pinned, checksum-verified PDFium for
this machine, then build and test:

```sh
./scripts/fetch_pdfium.sh
cargo build --release
SCRIPLITE_REQUIRE_PDFIUM=1 cargo test
```

End-to-end tests run against `tests/fixtures/public/sample-screenplay.pdf`, a
short original screenplay produced by `cargo run --example sample_screenplay`;
regenerate it after changing the example, or the tests will flag it as stale.
To try the tool on it:

```sh
scriplite inspect tests/fixtures/public/sample-screenplay.pdf
```

The tests in `tests/real_world.rs` use a private screenplay that is not
distributed with the source, and skip when it is absent.

To build a release archive in `dist/` (default: this machine's platform):

```sh
./scripts/package.sh linux-x64
```

Linux archives are built with [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)
against glibc 2.17, so they can be cross-built from macOS:
`cargo install --locked cargo-zigbuild` and
`rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu`.

Pushing a tag such as `v0.1.0` that matches the `Cargo.toml` version runs
`.github/workflows/release.yml`, which builds all four archives and attaches
them to a GitHub release.

At runtime, PDFium is resolved in this order:

1. `PDFIUM_LIB_PATH` (a library file or containing directory)
2. the directory containing `scriplite`
3. the current directory, then `vendor/pdfium/<platform>/lib/` beneath it
4. the system library search path

## Usage

Inspect without producing PDFs:

```sh
scriplite inspect "Dee Clock - 2026-07-25.pdf"
scriplite inspect "Dee Clock - 2026-07-25.pdf" --json report.json
```

Generate all individual speakers:

```sh
scriplite generate "Dee Clock - 2026-07-25.pdf" \
  --output-dir highlighted
```

Generate selected speakers or choose another RGB color:

```sh
scriplite generate "Dee Clock - 2026-07-25.pdf" \
  --output-dir highlighted \
  --character Mike \
  --character "Fru Dyck" \
  --color FEFE7F
```

Existing batch targets cause exit code 5 unless `--force` is supplied. Generation
is staged and verified before the complete batch is committed.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 2 | Invalid CLI usage |
| 3 | Input or PDF engine failure |
| 4 | Screenplay validation failure or unknown character |
| 5 | Existing destination collision |
| 6 | Generation, verification, or transaction failure |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Release archives also bundle PDFium,
whose licenses are included under `licenses/pdfium/`.
