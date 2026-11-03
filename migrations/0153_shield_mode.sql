-- Shield mode (docs/LIVE_STREAMS.md "Shield mode"): while a channel's shield is up
-- (chat_settings.followers_only_until in the future), incoming raids are refused.
CREATE OR REPLACE FUNCTION viewer_send_refusal(from_id text, to_id text, hosting boolean) RETURNS text
LANGUAGE sql STABLE AS $$
    SELECT CASE
        WHEN from_id = to_id THEN 'self'
        WHEN NOT coalesce((SELECT eligible FROM channel_users WHERE id = to_id), false) THEN 'unavailable'
        WHEN NOT coalesce((SELECT eligible FROM channel_users WHERE id = from_id), false) THEN 'unavailable'
        WHEN coalesce((SELECT CASE WHEN hosting THEN NOT accept_hosts ELSE NOT accept_raids END
                       FROM channel_host_settings WHERE user_id = to_id), false) THEN 'closed'
        WHEN NOT hosting
          AND EXISTS (SELECT 1 FROM chat_settings WHERE channel_id = to_id AND followers_only_until > now())
            THEN 'shielded'
        WHEN NOT hosting
          AND coalesce((SELECT raids_followed_only FROM channel_host_settings WHERE user_id = to_id), false)
          AND NOT EXISTS (SELECT 1 FROM follows WHERE follower_id = to_id AND following_id = from_id)
            THEN 'followed_only'
        WHEN EXISTS (SELECT 1 FROM raid_blocks WHERE channel_id = to_id AND blocked_id = from_id)
          OR EXISTS (SELECT 1 FROM user_blocks WHERE (blocker_id = to_id AND blocked_id = from_id)
                                                OR (blocker_id = from_id AND blocked_id = to_id))
          OR EXISTS (SELECT 1 FROM channel_restrictions WHERE channel_id = to_id AND user_id = from_id AND kind = 'ban')
            THEN 'blocked'
    END
$$;
