ALTER TABLE acl_member
    ADD CONSTRAINT acl_member_type_check
        CHECK (member_type IN ('character', 'corporation', 'alliance')),
    ADD CONSTRAINT acl_member_permission_check
        CHECK (permission IN ('read', 'read_write', 'manage', 'admin', 'deny')),
    ADD CONSTRAINT acl_member_role_for_type
        CHECK (member_type = 'character' OR permission NOT IN ('manage', 'admin'));
