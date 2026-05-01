FROM rust:1.92-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && groupadd --system --gid 10001 altair \
  && useradd --system --uid 10001 --gid altair --home-dir /nonexistent --shell /usr/sbin/nologin altair \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder --chown=altair:altair /app/target/release/altair-lab-api-service /app/altair-lab-api-service

EXPOSE 8085

ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

USER 10001

CMD ["/app/altair-lab-api-service"]
