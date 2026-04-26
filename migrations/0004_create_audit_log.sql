CREATE TABLE audit_log (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    occurred_at      TIMESTAMPTZ NOT NULL    DEFAULT now(),
    actor_account_id UUID        REFERENCES account(id) ON DELETE SET NULL,
    event_type       TEXT        NOT NULL,
    details          JSONB       NOT NULL    DEFAULT '{}'
);

CREATE INDEX audit_log_occurred_at_idx ON audit_log (occurred_at DESC);
CREATE INDEX audit_log_actor_idx       ON audit_log (actor_account_id)
    WHERE actor_account_id IS NOT NULL;
