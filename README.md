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
