ALTER TABLE account
    ADD CONSTRAINT account_status_check
        CHECK (status IN ('active', 'pending_delete'));
