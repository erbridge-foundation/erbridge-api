CREATE TABLE api_key (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    scope         TEXT        NOT NULL,
    account_id    UUID        REFERENCES account(id) ON DELETE CASCADE,
    name          TEXT        NOT NULL,
    key_hash      TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT api_key_scope_check CHECK (scope IN ('account'))
);

CREATE UNIQUE INDEX api_key_hash_idx ON api_key (key_hash);
CREATE INDEX api_key_account_idx ON api_key (account_id) WHERE account_id IS NOT NULL;
