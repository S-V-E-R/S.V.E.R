-- Staff sliding window (Joe, October 4, 2026). Staff actions are unlocked for 15 minutes after a
-- primary sign-in, a password confirmation or an authenticator-code confirmation, and each staff
-- action extends that by another 15 minutes, up to 8 hours from the confirmation. An authenticator
-- confirmation unlocks staff actions only; account-security changes keep their own 5-minute rule.
ALTER TABLE sessions
    ADD COLUMN staff_confirmed_at timestamptz,
    ADD COLUMN staff_active_at timestamptz;
