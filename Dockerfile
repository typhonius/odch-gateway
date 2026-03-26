# -- Builder stage --
FROM rust:1.83-slim AS builder

WORKDIR /usr/src/odch-gateway

# Pre-fetch dependencies by copying manifests first (layer caching)
COPY Cargo.toml Cargo.lock* ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy real source and build
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# -- Runtime stage --
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/odch-gateway/target/release/odch-gateway /usr/local/bin/odch-gateway

EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/odch-gateway"]
