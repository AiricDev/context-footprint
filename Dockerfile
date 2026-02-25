# -----------------------------------------------------------------------------
# Stage 1: Build Rust cftool (no SCIP/protobuf; semantic data from JSON)
# -----------------------------------------------------------------------------
FROM rust:1-bookworm AS rust-builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app
COPY . .

RUN cargo build --release

# -----------------------------------------------------------------------------
# Stage 2: Final image with cftool + Bun + extractors
# -----------------------------------------------------------------------------
FROM oven/bun:1-debian

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/extractors
COPY extractors/ ./
RUN bun install --frozen-lockfile

# cftool binary
COPY --from=rust-builder /usr/src/app/target/release/cftool /usr/local/bin/cftool

# Default: run cftool (expects semantic JSON path as first arg)
# To extract semantics: docker run --entrypoint bun <image> run src/cli.ts python /path/to/project -o /out/semantic.json
ENTRYPOINT ["cftool"]
