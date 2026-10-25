-- Activity in the OBS dock (docs/OBS.md): every alert a channel received (follow, subscription,
-- tribute, Skill, incoming raid), written beside its live event so the dock lists them and can
-- replay one. `data` is the live event's own data, so a replay is the same alert. Kept 30 days;
-- a tribute's message goes after 7, like the chat message it came from.
CREATE TABLE stream_activity (
    id bigserial PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('follow', 'sub', 'tribute', 'skill', 'raid')),
    data jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX stream_activity_channel ON stream_activity (channel_id, id DESC);
CREATE INDEX stream_activity_age ON stream_activity (created_at);

-- Start with what the lasting records still hold: counted follows and incoming raids from the last
-- 30 days, tributes and Skills from chat (kept 7 days). Subscription months have no history to
-- read, so they start now.
INSERT INTO stream_activity (channel_id, kind, data, created_at)
SELECT channel_id, kind, data, created_at FROM (
    SELECT f.following_id AS channel_id, 'follow' AS kind, jsonb_build_object('user', u.username) AS data, f.counted_at AS created_at
    FROM follows f JOIN users u ON u.id = f.follower_id
    WHERE f.counted_at > now() - interval '30 days'
    UNION ALL
    SELECT r.target_id, 'raid', jsonb_build_object('from', u.username, 'seconds', 0), r.execute_at
    FROM raids r JOIN users u ON u.id = r.raider_id
    WHERE r.status = 'moved' AND r.execute_at > now() - interval '30 days'
    UNION ALL
    SELECT m.channel_id, 'tribute', jsonb_build_object('user', u.username, 'valor', m.tribute, 'message', m.body), m.created_at
    FROM chat_messages m JOIN users u ON u.id = m.author_id
    WHERE m.tribute IS NOT NULL AND m.channel_id IS NOT NULL AND m.squad_id IS NULL AND m.deleted_at IS NULL
    UNION ALL
    SELECT m.channel_id, 'skill', jsonb_build_object('user', u.username, 'skill', m.skill, 'valor', m.tribute), m.created_at
    FROM chat_messages m JOIN users u ON u.id = m.author_id
    WHERE m.skill IS NOT NULL AND m.tribute IS NOT NULL AND m.channel_id IS NOT NULL AND m.squad_id IS NULL AND m.deleted_at IS NULL
) backfill
WHERE NOT EXISTS (
    SELECT 1 FROM user_blocks k JOIN users b ON b.id = k.blocked_id
    WHERE k.blocker_id = backfill.channel_id AND lower(b.username) = lower(coalesce(backfill.data->>'user', backfill.data->>'from'))
)
ORDER BY created_at;
