-- Module 5 MAGNet hardening (docs/MAGNET.md "Hardening"): who counts toward a stream's moment
-- signals, and lane health for staff.

-- A real viewer of these broadcasts: a session viewer integrity counted at some point and does not
-- exclude now. People MAGNet itself brought to the stream never count, so being featured can't
-- keep a stream featured.
CREATE FUNCTION magnet_room_viewer(watched text[], person text) RETURNS boolean
LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM playback_leases l WHERE l.broadcast_id = ANY(watched)
            AND l.viewer_key = 'u:' || person AND l.ever_counted AND l.level <> 'excluded')
        AND NOT EXISTS(SELECT 1 FROM playback_leases l WHERE l.broadcast_id = ANY(watched)
            AND l.viewer_key = 'u:' || person AND l.magnet_lane IS NOT NULL)
$$;

-- Low-effort chat counts for less in a burst: a message made only of the room's emotes, or the
-- same text another chatter sent in the same room within a minute (copy-paste). Parameter names
-- differ from every column name, which would otherwise shadow them.
CREATE FUNCTION magnet_low_effort(room text, room_squad text, sender text, said text, sent_at timestamptz)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS(SELECT 1 FROM regexp_split_to_table(btrim(said), '\s+') w
            WHERE NOT EXISTS(SELECT 1 FROM channel_emotes e
                WHERE e.channel_id = room AND e.status = 'VISIBLE' AND e.code = w))
        OR EXISTS(SELECT 1 FROM chat_messages o
            WHERE CASE WHEN room_squad IS NULL THEN o.channel_id = room AND o.squad_id IS NULL
                ELSE o.squad_id = room_squad END
            AND o.author_id <> sender AND o.deleted_at IS NULL AND o.origin IS NULL
            AND o.created_at BETWEEN sent_at - interval '60 seconds' AND sent_at + interval '60 seconds'
            AND lower(btrim(o.body)) = lower(btrim(said)))
$$;

-- The signal queries read recent messages per room by time.
CREATE INDEX chat_messages_recent ON chat_messages (channel_id, created_at) WHERE squad_id IS NULL;
CREATE INDEX chat_messages_squad_recent ON chat_messages (squad_id, created_at) WHERE squad_id IS NOT NULL;

-- Lane health: a lane whose ticks keep failing holds its stream; staff see since when.
ALTER TABLE magnet_lanes
    ADD COLUMN failing_since timestamptz,
    ADD COLUMN failures integer NOT NULL DEFAULT 0;
