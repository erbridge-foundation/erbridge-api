-- Remove 'tentative' from the connection status CHECK constraint.
-- Both ends are always inserted atomically so a connection can never have
-- fewer than 2 ends; tentative is unreachable by design.
ALTER TABLE map_connections
    DROP CONSTRAINT IF EXISTS map_connections_status_check;

ALTER TABLE map_connections
    ADD CONSTRAINT map_connections_status_check
    CHECK (status IN ('partial','linked','fully_linked','collapsed','expired'));
