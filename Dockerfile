# Step 1: Builder
FROM rustlang/rust:nightly AS builder
WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY . .

# Build the binaries
RUN cargo build --release --bin flashstat-server --bin flashstat

# Step 2: Runtime
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install SSL certs for any external API calls
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy binaries from builder
COPY --from=builder /app/target/release/flashstat-server /usr/local/bin/
COPY --from=builder /app/target/release/flashstat /usr/local/bin/

# Default config environment variables
ENV FLASHSTAT_HOST=0.0.0.0
ENV FLASHSTAT_PORT=9944
ENV RUST_LOG=info

# Expose RPC port
EXPOSE 9944

# Run the server by default
CMD ["flashstat-server"]
