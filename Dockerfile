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
# Stage 2: Install Bun-based extractor dependencies
# -----------------------------------------------------------------------------
FROM oven/bun:1-debian AS extractor-builder

WORKDIR /app/extractors

COPY extractors/package.json extractors/bun.lock* ./
RUN bun install --frozen-lockfile

COPY extractors/ ./
# No separate build step; Bun runs TypeScript directly

# -----------------------------------------------------------------------------
# Stage 3: Final image with cftool + Bun + extractors
# -----------------------------------------------------------------------------
FROM oven/bun:1-debian

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# cftool binary
COPY --from=rust-builder /usr/src/app/target/release/cftool /usr/local/bin/cftool

# Bun-based extractor (LSP-powered semantic data extraction)
COPY --from=extractor-builder /app/extractors /app/extractors
WORKDIR /app/extractors

# Default: run cftool (expects semantic JSON path as first arg)
# To extract semantics: docker run --entrypoint bun <image> run src/cli.ts python /path/to/project -o /out/semantic.json
ENTRYPOINT ["cftool"]
