CREATE OR REPLACE VIEW system_edges AS
SELECT
    c.map_id,
    c.connection_id,
    c.status,
    c.life_state,
    c.mass_state,
    a.system_id  AS from_system_id,
    b.system_id  AS to_system_id,
    a.signature_id AS from_signature_id,
    b.signature_id AS to_signature_id,
    c.updated_at
FROM map_connections c
JOIN map_connection_ends a ON a.connection_id = c.connection_id AND a.side = 'a'
JOIN map_connection_ends b ON b.connection_id = c.connection_id AND b.side = 'b'
UNION ALL
SELECT
    c.map_id,
    c.connection_id,
    c.status,
    c.life_state,
    c.mass_state,
    b.system_id  AS from_system_id,
    a.system_id  AS to_system_id,
    b.signature_id AS from_signature_id,
    a.signature_id AS to_signature_id,
    c.updated_at
FROM map_connections c
JOIN map_connection_ends a ON a.connection_id = c.connection_id AND a.side = 'a'
JOIN map_connection_ends b ON b.connection_id = c.connection_id AND b.side = 'b';
