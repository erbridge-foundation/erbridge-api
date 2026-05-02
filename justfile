set dotenv-load

image := "ghcr.io/erbridge-foundation/erbridge-api"

# List available recipes
default:
    @just --list

# ── Development ───────────────────────────────────────────────────────────────

# Run the API (with hot-reload via cargo-watch if available)
dev:
    cargo watch -x run

# Run the API once
run:
    cargo run

# Run the full stack (API + database via Docker Compose)
up:
    docker compose up

# Start only the database in the background
db:
    docker compose up -d db

# Stop all Compose services
down:
    docker compose down

# ── Code quality ──────────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test

# Run a specific test by name (substring match)
test-one name:
    cargo test {{name}}

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Check formatting
fmt-check:
    cargo fmt --check

# Apply formatting
fmt:
    cargo fmt

# Run all checks (fmt, lint, test) — mirrors CI
check: fmt-check lint test

# ── sqlx ──────────────────────────────────────────────────────────────────────

# Run pending database migrations against DATABASE_URL
migrate:
    cargo sqlx migrate run

# Regenerate the .sqlx offline query cache (run after changing any sqlx::query! macro)
prepare:
    cargo sqlx prepare -- --tests --features dev-seed

# Verify the .sqlx cache is up to date (same check CI runs)
prepare-check:
    cargo sqlx prepare --check -- --tests --features dev-seed

# ── Build ─────────────────────────────────────────────────────────────────────

# Build a release binary
build:
    cargo build --release

# Build the Docker image locally
docker-build tag="dev":
    docker build --build-arg VERSION={{tag}} -t {{image}}:{{tag}} .

# ── Registry ──────────────────────────────────────────────────────────────────

# Push a locally built image to GHCR (requires prior `docker login ghcr.io`)
docker-push tag="dev":
    docker push {{image}}:{{tag}}

# Build and push in one step
docker-release tag="dev": (docker-build tag) (docker-push tag)

# ── Utilities ─────────────────────────────────────────────────────────────────

# Print the current version from Cargo.toml
version:
    @cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version'

# Tail API logs from a running Compose stack
logs:
    docker compose logs -f api

# Open a psql session against the local database
psql:
    docker compose exec db psql -U erbridge -d erbridge

# Remove the local database volume (destructive — loses all local data)
db-reset:
    docker compose down -v
