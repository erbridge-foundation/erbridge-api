CREATE TABLE map_events (
    seq         BIGSERIAL   PRIMARY KEY,
    map_id      UUID        NOT NULL REFERENCES maps(map_id) ON DELETE CASCADE,
    entity_type TEXT        NOT NULL,
    entity_id   TEXT        NOT NULL,
    event_type  TEXT        NOT NULL,
    event_time  TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_id    TEXT,
    payload     JSONB       NOT NULL DEFAULT '{}'
);

CREATE INDEX map_events_map_seq_idx  ON map_events (map_id, seq);
CREATE INDEX map_events_map_time_idx ON map_events (map_id, event_time);

CREATE TABLE map_checkpoints (
    checkpoint_id     BIGSERIAL   PRIMARY KEY,
    map_id            UUID        NOT NULL REFERENCES maps(map_id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_included_seq BIGINT      NOT NULL,
    checkpoint_version INT        NOT NULL DEFAULT 1,
    event_count       INT,
    checksum          TEXT,
    state             JSONB       NOT NULL,
    UNIQUE (map_id, last_included_seq)
);

CREATE INDEX map_checkpoints_map_seq_idx ON map_checkpoints (map_id, last_included_seq DESC);
