FROM rust:1.75-slim-bookworm as builder

WORKDIR /app

# Install dependencies for native-tls
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy everything and build
COPY . .

RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/latency-auditor /app/latency-auditor

ENV RUST_LOG=info

CMD ["/app/latency-auditor"]
