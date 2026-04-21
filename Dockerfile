FROM rust:1.88-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/referral-code-role target/release/deps/referral_code_role*

COPY src/ src/
COPY migrations/ migrations/
COPY favicon.ico favicon.ico
RUN cargo build --release && strip target/release/referral-code-role

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/referral-code-role /usr/local/bin/
EXPOSE 8080
CMD ["referral-code-role"]
