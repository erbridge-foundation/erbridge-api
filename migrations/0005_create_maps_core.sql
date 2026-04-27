CREATE TABLE map (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT        NOT NULL,
    slug                TEXT        NOT NULL UNIQUE,
    owner_account_id    UUID        REFERENCES account(id) ON DELETE SET NULL,
    description         TEXT,
    deleted             BOOLEAN     NOT NULL DEFAULT false,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_checkpoint_seq BIGINT      NOT NULL DEFAULT 0,
    last_checkpoint_at  TIMESTAMPTZ,
    retention_days      INT         NOT NULL DEFAULT 14
);

CREATE INDEX map_owner_idx ON map (owner_account_id);

CREATE TABLE map_connections (
    connection_id UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    map_id        UUID        NOT NULL REFERENCES map(id) ON DELETE CASCADE,
    status        TEXT        NOT NULL DEFAULT 'partial'
                      CHECK (status IN ('partial','linked','fully_linked','collapsed','expired')),
    life_state    TEXT        CHECK (life_state IN ('fresh','eol') OR life_state IS NULL),
    mass_state    TEXT        CHECK (mass_state IN ('stable','reduced','critical') OR mass_state IS NULL),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    extra         JSONB       NOT NULL DEFAULT '{}'
);

CREATE INDEX map_connections_map_idx          ON map_connections (map_id);
CREATE INDEX map_connections_map_status_idx   ON map_connections (map_id, status);
CREATE INDEX map_connections_route_filter_idx ON map_connections (map_id, status, life_state, mass_state);

CREATE TABLE map_connection_ends (
    connection_id UUID   NOT NULL REFERENCES map_connections(connection_id) ON DELETE CASCADE,
    side          TEXT   NOT NULL CHECK (side IN ('a','b')),
    system_id     BIGINT NOT NULL REFERENCES sde_solar_system(solar_system_id),
    signature_id  UUID   UNIQUE,
    wormhole_code TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (connection_id, side)
);

CREATE INDEX map_connection_ends_system_idx            ON map_connection_ends (system_id);
CREATE INDEX map_connection_ends_system_connection_idx ON map_connection_ends (system_id, connection_id);

CREATE TABLE map_signatures (
    signature_id        UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    map_id              UUID        NOT NULL REFERENCES map(id) ON DELETE CASCADE,
    system_id           BIGINT      NOT NULL REFERENCES sde_solar_system(solar_system_id),
    sig_code            TEXT        NOT NULL,
    sig_type            TEXT        NOT NULL,
    status              TEXT        NOT NULL DEFAULT 'visible'
                            CHECK (status IN ('visible','resolved','expired','deleted')),
    connection_id       UUID,
    connection_side     TEXT        CHECK (connection_side IN ('a','b') OR connection_side IS NULL),
    wormhole_code       TEXT,
    derived_life_state  TEXT        CHECK (derived_life_state IN ('fresh','eol') OR derived_life_state IS NULL),
    derived_mass_state  TEXT        CHECK (derived_mass_state IN ('stable','reduced','critical') OR derived_mass_state IS NULL),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    extra               JSONB       NOT NULL DEFAULT '{}'
);

CREATE INDEX map_signatures_map_system_idx     ON map_signatures (map_id, system_id);
CREATE INDEX map_signatures_map_connection_idx ON map_signatures (map_id, connection_id);
CREATE UNIQUE INDEX map_signatures_active_code_uidx
    ON map_signatures (map_id, system_id, sig_code)
    WHERE status IN ('visible','resolved');

ALTER TABLE map_connection_ends
    ADD CONSTRAINT map_connection_ends_signature_fk
    FOREIGN KEY (signature_id) REFERENCES map_signatures(signature_id) ON DELETE SET NULL;

ALTER TABLE map_signatures
    ADD CONSTRAINT map_signatures_connection_end_fk
    FOREIGN KEY (connection_id, connection_side)
    REFERENCES map_connection_ends(connection_id, side)
    DEFERRABLE INITIALLY DEFERRED;
