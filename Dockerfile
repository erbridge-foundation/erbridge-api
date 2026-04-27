FROM rust:1.95-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

ARG VERSION=dev
COPY Cargo.toml Cargo.lock ./
RUN sed -i "s/^version = \".*\"/version = \"${VERSION}\"/" Cargo.toml
COPY .sqlx ./.sqlx
# Pre-build dependencies with a dummy main so they are cached in a separate layer
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && SQLX_OFFLINE=true cargo build --release \
    && rm -rf src

COPY . .
# SQLX_OFFLINE uses the checked-in .sqlx cache — no live DB needed at build time
RUN touch src/main.rs && SQLX_OFFLINE=true cargo build --release

FROM alpine:3.21

RUN apk add --no-cache ca-certificates wget

WORKDIR /app

COPY --from=builder /app/target/release/erbridge-api ./erbridge-api
COPY --from=builder /app/migrations ./migrations

EXPOSE 8080

CMD ["./erbridge-api"]
