# Build:  docker build -t scriplite .
# Run:    docker run --rm -v "$PWD:/work" scriplite inspect "script.pdf"
FROM rust:1.94-bookworm AS build
WORKDIR /src
COPY scripts/fetch_pdfium.sh scripts/
RUN ./scripts/fetch_pdfium.sh >/tmp/pdfium-path
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --locked --release \
    && mkdir /out \
    && cp target/release/scriplite "$(cat /tmp/pdfium-path)" /out/ \
    && pdfium_dir=$(dirname "$(dirname "$(cat /tmp/pdfium-path)")") \
    && mkdir -p /out/licenses/pdfium \
    && cp "$pdfium_dir/LICENSE" "$pdfium_dir"/licenses/* /out/licenses/pdfium/

FROM debian:bookworm-slim
COPY --from=build /out/ /opt/scriplite/
RUN useradd --create-home --uid 1000 scriplite
USER scriplite
WORKDIR /work
ENV PDFIUM_LIB_PATH=/opt/scriplite
ENTRYPOINT ["/opt/scriplite/scriplite"]
CMD ["--help"]
