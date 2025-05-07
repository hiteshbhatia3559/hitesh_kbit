FROM rust:1.86 AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    libbsd-dev \
    uuid-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the Cargo.toml file first
COPY Cargo.toml ./

# Create a dummy main.rs to build dependencies
RUN mkdir -p src && echo "fn main() { println!(\"dummy\"); }" > src/main.rs

# Build dependencies
RUN cargo build --release || true

# Copy the SDK separately
COPY hyperliquid-rust-sdk ./hyperliquid-rust-sdk

# Update workspace member references
RUN sed -i 's|"hyperliquid-rust-sdk"|"./hyperliquid-rust-sdk"|g' Cargo.toml

# Now try to build dependencies again with proper workspace path
RUN cargo build --release || true

# Now copy the actual source code
COPY src ./src

# Build the application
RUN cargo build --release

# Create a minimal runtime image
FROM debian:bullseye-slim

WORKDIR /app

# Install necessary runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    libbsd0 \
    uuid \
    && rm -rf /var/lib/apt/lists/*

# Create config directory
RUN mkdir -p /app/config

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/hyperliquid-market-maker /app/hyperliquid-market-maker

# Set the environment variables
ENV RUST_LOG=info
ENV REDIS_URL=redis://redis:6379
# HYPERLIQUID_WALLET_KEY should be provided at runtime

# Set the command to run the binary
CMD ["/app/hyperliquid-market-maker"] 