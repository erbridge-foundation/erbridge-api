-- D1: index on eve_character.account_id for frequent filter queries
CREATE INDEX eve_character_account_id_idx ON eve_character (account_id);

-- D2: indexes on acl_member for FK cascade performance
CREATE INDEX acl_member_acl_id_idx       ON acl_member (acl_id);
CREATE INDEX acl_member_character_id_idx ON acl_member (character_id) WHERE character_id IS NOT NULL;

-- D3: index on map_acl.acl_id for detach orphan check
CREATE INDEX map_acl_acl_id_idx ON map_acl (acl_id);

-- D4: index on audit_log.event_type for analytics queries
CREATE INDEX audit_log_event_type_idx ON audit_log (event_type);
