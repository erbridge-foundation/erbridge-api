# ER Bridge

[![Build](https://github.com/erbridge-foundation/erbridge-api/actions/workflows/build.yml/badge.svg)](https://github.com/erbridge-foundation/erbridge-api/actions/workflows/build.yml)

---

> **⚠️ Work in progress — not usable yet.**
> ER Bridge is under active development. It is not currently functional as a wormhole mapper. Expect missing features, breaking changes, and rough edges. Follow along or contribute, but don't try to deploy this for actual use yet.

---

**An EVE Online Wormhole Mapper**

> _"Every hole has two sides."_

**Pronounced:** _ur-bridge_ — the "ER" is read as /ɜː/, rhyming with "her" or "stir". Not "ER" as individual letters, not "ear". Just _ur_.

ER Bridge is an open-source wormhole mapping tool for EVE Online corporations and alliances. Track your chain, tag connections, share situational awareness with your fleet, and never lose your way back to known space again.

---

## Features

- 🕳️ **Live chain mapping** — visualise your wormhole chain in real time
- 🕰️ **History Mode** — see what your map looked like up to 48 hours ago
- 🔗 **Connection tracking** — record mass status, ship transits, mass and lifetime state
- 👥 **Multi-account support** — share your map with allies and friends
- 🔍 **System signatures** — log and annotate signatures per system
- 📡 **ESI integration** — pull character and corp data via the EVE Swagger Interface
- 🗺️ **Wormhole data** — statics, effect, class, and region information at a glance

---

## Self-hosting

### Prerequisites

- Docker and Docker Compose
- A registered ESI application — create one at the [EVE Developer Portal](https://developers.eveonline.com/)
  - Set the callback URL to `https://<your-domain>/auth/callback`

### Setup

**1. Create a `docker-compose.yml`**

```yaml
services:
  caddy:
    image: caddy:2-alpine
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile
      - caddy_data:/data
    depends_on:
      - backend
      - frontend

  backend:
    image: ghcr.io/erbridge-foundation/erbridge-backend:latest
    environment:
      APP_URL: https://<your-domain>
      DATABASE_URL: postgres://erbridge:password@db:5432/erbridge
      ENCRYPTION_SECRET: <32-byte hex secret>
      ESI_CLIENT_ID: <your ESI client ID>
      ESI_CLIENT_SECRET: <your ESI client secret>
    depends_on:
      - db

  frontend:
    image: ghcr.io/erbridge-foundation/erbridge-frontend:latest
    environment:
      APP_URL: https://<your-domain>

  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: erbridge
      POSTGRES_USER: erbridge
      POSTGRES_PASSWORD: password
    volumes:
      - db_data:/var/lib/postgresql/data

volumes:
  caddy_data:
  db_data:
```

**2. Create a `Caddyfile`**

```
<your-domain> {
    reverse_proxy /auth/* backend:8080
    reverse_proxy /api/*  backend:8080
    reverse_proxy /sse    backend:8080
    reverse_proxy *       frontend:3000
}
```

**3. Generate an encryption secret**

```sh
openssl rand -hex 32
```

**4. Start the stack**

```sh
docker compose up -d
```

The database schema is applied automatically on first startup.

### Configuration reference

| Variable            | Required | Description                                                                      |
| ------------------- | -------- | -------------------------------------------------------------------------------- |
| `APP_URL`           | ✅       | Public-facing base URL, no trailing slash (e.g. `https://erbridge.example.com`) |
| `DATABASE_URL`      | ✅       | PostgreSQL connection string (e.g. `postgres://user:pass@db:5432/erbridge`)      |
| `ENCRYPTION_SECRET` | ✅       | 32-byte hex secret for AES-256-GCM encryption and JWT signing                   |
| `ESI_CLIENT_ID`     | ✅       | Your ESI application client ID                                                   |
| `ESI_CLIENT_SECRET` | ✅       | Your ESI application secret                                                      |
| `ESI_CALLBACK_URL`  | ⬜       | OAuth callback URL — defaults to `APP_URL/auth/callback`                         |
| `FRONTEND_URL`      | ⬜       | Post-login redirect URL — defaults to `APP_URL`                                  |

---

## Integration tests (hurl)

The `hurl/` directory contains HTTP integration tests that run against a live server backed by a real database. They cover accounts, API-key lifecycle, ACL CRUD, map/connection/signature CRUD, and admin routes.

> **⚠️ These tests wipe the database on every run.**
> They are designed for CI and for local development against a disposable database.
> Never point them at a database that holds real data.

### Running locally

```sh
./scripts/hurl-test.sh
```

The script:
1. Drops and recreates all tables in the local database (using `DATABASE_URL` from `.env`).
2. Re-runs migrations via `sqlx migrate run`.
3. Builds the binary with `--features dev-seed`.
4. Starts the server (which seeds two test accounts on boot).
5. Runs all `hurl/*.hurl` files with the dev-seed API keys as variables.
6. Stops the server on exit.

### Variables

The hurl files use three variables that the script supplies automatically:

| Variable | Description |
|---|---|
| `base_url` | Server base URL (default `http://localhost:8080`) |
| `admin_api_key` | API key for the seeded admin account |
| `user_api_key` | API key for the seeded non-admin account |

You can override the server URL:

```sh
BASE_URL=http://localhost:9090 ./scripts/hurl-test.sh
```

Or run hurl directly against an already-running server (DB already seeded):

```sh
hurl --test \
  --variable base_url=http://localhost:8080 \
  --variable admin_api_key=erbridge_<32hex> \
  --variable user_api_key=erbridge_<32hex> \
  hurl/*.hurl
```

### What is and isn't tested

| Covered | Not covered (requires live ESI) |
|---|---|
| Health check | `POST /auth/callback` (OAuth flow) |
| Account/me + character list | ESI token refresh |
| API key create/list/revoke/auth | Online & location poller endpoints |
| ACL CRUD + member add/update/remove | |
| Map CRUD + connections/signatures | |
| Route finding | |
| Admin routes (auth wiring + role gate) | |

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for development setup and contribution guidelines.

---

## License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

See [LICENSE](./LICENSE) for the full text.

### Why AGPL?

ER Bridge is a web application. The AGPL-3.0 was chosen deliberately: if you run a **modified version of this software as a network service**, you are required to make your modifications available under the same license.

**What this means in practice:**

- ✅ You can use ER Bridge freely for your personal, corp, or alliance use
- ✅ You can fork and modify it
- ✅ You can self-host it for your group
- ⚠️ If you host a **modified** version for others (even privately over a network), you must publish your source changes under AGPL-3.0
- ❌ You cannot take this code proprietary and offer it as a closed-source service

---

## EVE Online Third-Party Developer Notice

ER Bridge is an independent, community-built tool and is **not affiliated with, endorsed by, or supported by CCP Games**.

EVE Online, the EVE logo, and all related names, marks, and imagery are the intellectual property of **CCP hf.**. Use of EVE-related data in this project is governed by the [EVE Online Developer License Agreement](https://developers.eveonline.com/license-agreement) and the [ESI terms of use](https://developers.eveonline.com/terms-of-use).

Please review CCP's developer terms before deploying a public or commercial instance of this tool.

---

## Acknowledgements

Inspired by Tripwire and Wanderer. Special thanks to the wormhole community for keeping J-space weird.

---

_EVE Online is a trademark of CCP hf. All rights reserved._
