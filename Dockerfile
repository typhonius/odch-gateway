# -- Builder stage --
FROM rust:1-slim AS builder

WORKDIR /usr/src/odch-gateway

# Pre-fetch dependencies by copying manifests first (layer caching)
COPY Cargo.toml Cargo.lock* ./

# Create a dummy main.rs and empty admin-ui to build dependencies
RUN mkdir src && echo 'fn main() {}' > src/main.rs && mkdir -p admin-ui
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src admin-ui

# Copy real source and static assets, then build
COPY src/ src/
COPY admin-ui/ admin-ui/
RUN touch src/main.rs && cargo build --release

# -- Runtime stage --
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root user for security
RUN groupadd --system gateway && useradd --system --gid gateway gateway

COPY --from=builder /usr/src/odch-gateway/target/release/odch-gateway /usr/local/bin/odch-gateway

USER gateway

EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/odch-gateway"]
