CREATE TABLE acl_member (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    acl_id        UUID        NOT NULL REFERENCES acl(id) ON DELETE CASCADE,
    member_type   TEXT        NOT NULL,
    eve_entity_id BIGINT,
    character_id  UUID        REFERENCES eve_character(id) ON DELETE CASCADE,
    name          TEXT        NOT NULL DEFAULT '',
    permission    TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
