CREATE TABLE account (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    status              TEXT        NOT NULL DEFAULT 'active',
    delete_requested_at TIMESTAMPTZ,
    is_server_admin 		BOOLEAN 		NOT NULL DEFAULT FALSE
);

CREATE INDEX account_server_admin_idx ON account (id) WHERE is_server_admin = TRUE;
