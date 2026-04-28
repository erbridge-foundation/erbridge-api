ALTER TABLE account ADD COLUMN is_server_admin BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX account_server_admin_idx ON account (id) WHERE is_server_admin = TRUE;
