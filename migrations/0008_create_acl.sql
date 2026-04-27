-- owner_account_id is intentionally unindexed: "list all ACLs owned by X" is not a hot query path
CREATE TABLE acl (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT        NOT NULL,
    owner_account_id  UUID        REFERENCES account(id) ON DELETE SET NULL,
    pending_delete_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
