FROM rust:1.77 as builder

WORKDIR /usr/src/app

# Copier les fichiers Cargo pour optimiser le cache
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# ---- Runtime ----
FROM debian:stable-slim

# On copie seulement le binaire, pas les sources
COPY --from=builder /usr/src/app/target/release/altair-lab-api-service /usr/local/bin/lab-api

EXPOSE 8085

CMD ["lab-api"]
