-- Module 3: staff username resets for impersonation (docs/LIVE_STREAMS.md, approved October 3,
-- 2026). A reset is recorded in the account's username history with the reason shown to the
-- user. The old name is held without a redirect, so it neither points at the account nor can be
-- claimed by anyone else while the hold lasts.
ALTER TABLE username_history DROP CONSTRAINT username_history_reason_check;
ALTER TABLE username_history ADD CONSTRAINT username_history_reason_check
    CHECK (reason IN ('rename', 'revert', 'import', 'staff_reset'));
ALTER TABLE username_history ADD COLUMN note text NOT NULL DEFAULT '';
