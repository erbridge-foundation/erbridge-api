CREATE TABLE blocked_eve_character (
    eve_character_id  BIGINT      PRIMARY KEY REFERENCES eve_character(eve_character_id) ON DELETE CASCADE,
    reason            TEXT,
    blocked_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
