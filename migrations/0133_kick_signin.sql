-- Kick sign-in (Joe, October 20, 2026): the legacy app's Kick keys and callback. It also proves a
-- Kick channel is a member's own for Discord go-live alerts.
ALTER TABLE identities DROP CONSTRAINT identities_provider_check,
    ADD CONSTRAINT identities_provider_check CHECK (provider IN ('google', 'twitch', 'discord', 'kick'));
ALTER TABLE oauth_signups DROP CONSTRAINT oauth_signups_provider_check,
    ADD CONSTRAINT oauth_signups_provider_check CHECK (provider IN ('google', 'twitch', 'discord', 'kick'));

-- The legacy import kept Kick links aside until Kick sign-in existed; they're active now. A Kick
-- account already linked here, or a member already holding another Kick link, keeps what's there.
INSERT INTO identities(provider, subject, user_id)
SELECT DISTINCT ON (i->>'providerUserId') 'kick', i->>'providerUserId', d.user_id
FROM legacy_account_data d
CROSS JOIN jsonb_array_elements(d.identities) i
JOIN users u ON u.id = d.user_id
WHERE lower(i->>'provider') = 'kick' AND coalesce(i->>'providerUserId', '') <> ''
ORDER BY i->>'providerUserId', d.user_id
ON CONFLICT DO NOTHING;
