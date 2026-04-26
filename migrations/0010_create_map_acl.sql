CREATE TABLE map_acl (
    map_id      UUID        NOT NULL REFERENCES map(id) ON DELETE CASCADE,
    acl_id      UUID        NOT NULL REFERENCES acl(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (map_id, acl_id)
);
