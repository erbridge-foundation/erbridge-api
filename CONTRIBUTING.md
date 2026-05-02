# Contributing to erbridge-api

## Prerequisites

- Rust (stable toolchain)
- Docker and Docker Compose (for the local database)
- An ESI application registered at the [EVE Developer Portal](https://developers.eveonline.com/)
  - Set the callback URL to `http://localhost:8080/auth/callback`

## Local setup

**1. Clone and configure**

```sh
git clone https://github.com/erbridge-foundation/erbridge-api.git
cd erbridge-api
cp .env.sample .env
```

Edit `.env` and fill in `ENCRYPTION_SECRET`, `ESI_CLIENT_ID`, and `ESI_CLIENT_SECRET`.
The other values can stay as the defaults for local development.

**2. Create a local Compose file and start the database**

`docker-compose.yml` is gitignored — copy the sample and customise if needed:

```sh
cp docker-compose.sample.yml docker-compose.yml
just db
```

**3. Run the API**

```sh
just run
```

Migrations are applied automatically on startup. The first run also downloads the EVE SDE
(~60 MB), which may take a moment.

The API is available at `http://localhost:8080`. `GET /api/health` confirms it is up.

## Running the tests

```sh
just test
```

Integration tests spin up an embedded PostgreSQL instance and a WireMock server automatically —
no extra infrastructure is required.

## After changing SQL queries

sqlx verifies queries at compile time using a checked-in cache. After modifying any
`sqlx::query!` macro, regenerate the cache and commit it alongside your changes:

```sh
just prepare
```

The CI build runs with `SQLX_OFFLINE=true` and will fail if the cache is out of date.

## Before opening a PR

```sh
just check
```

This runs formatting, clippy, and tests in one step — the same checks CI runs.

## Guidelines

- Open an issue before starting significant work so we can align on the approach.
- Keep commits focused; one logical change per commit.
- Follow the patterns already in the codebase — see `CLAUDE.md` for a summary of key conventions.
