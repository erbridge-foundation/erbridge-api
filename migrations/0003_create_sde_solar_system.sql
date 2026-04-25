CREATE TABLE sde_solar_system (
    solar_system_id                 BIGINT          PRIMARY KEY,
    name                            TEXT            NOT NULL,
    region_id                       BIGINT,
    constellation_id                BIGINT,
    faction_id                      BIGINT,
    star_id                         BIGINT,
    security_status                 REAL,
    security_class                  TEXT,
    wh_class                        TEXT,
    wormhole_class_id               BIGINT,
    luminosity                      REAL,
    radius                          DOUBLE PRECISION,
    border                          BOOLEAN,
    corridor                        BOOLEAN,
    fringe                          BOOLEAN,
    hub                             BOOLEAN,
    international                   BOOLEAN,
    regional                        BOOLEAN,
    visual_effect                   TEXT,
    name_i18n                       JSONB,
    planet_ids                      JSONB,
    stargate_ids                    JSONB,
    disallowed_anchor_categories    JSONB,
    disallowed_anchor_groups        JSONB,
    position                        JSONB,
    position_2d                     JSONB
);

CREATE TABLE sde_solar_system_metadata (
    id              SMALLINT    PRIMARY KEY,
    sde_version     TEXT        NOT NULL,
    sde_checksum    TEXT        NOT NULL,
    loaded_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
