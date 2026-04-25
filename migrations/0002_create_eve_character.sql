CREATE TABLE eve_character (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id              UUID        REFERENCES account(id) ON DELETE CASCADE,
    eve_character_id        BIGINT      NOT NULL UNIQUE,
    name                    TEXT        NOT NULL,
    corporation_id          BIGINT      NOT NULL,
    alliance_id             BIGINT,
    is_main                 BOOLEAN     NOT NULL DEFAULT false,
    encrypted_access_token  BYTEA,
    encrypted_refresh_token BYTEA,
    esi_token_expires_at    TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX eve_character_one_main_per_account
    ON eve_character(account_id)
    WHERE is_main = true;
