FROM rust:latest as builder

WORKDIR /usr/src/app

# Ensuite copie le code source
COPY src ./src

# Copie uniquement Cargo.toml d'abord
COPY Cargo.toml ./

# Pré-télécharge les dépendances
RUN cargo fetch

# Compile en release
RUN cargo build --release

# ---- Runtime ----
FROM debian:stable-slim

# Copie le binaire seulement
COPY --from=builder /usr/src/app/target/release/altair-lab-api-service /usr/local/bin/lab-api

EXPOSE 8085

CMD ["lab-api"]
