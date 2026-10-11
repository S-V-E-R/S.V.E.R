-- Beacon comments (docs/BEACONS.md "Comments"; Joe's decision October 10, 2026). The author or the
-- Beacon's owner deletes one (deleted_at); staff removal is a status, restorable on appeal, like
-- wall replies.
CREATE TABLE beacon_comments (
    id text PRIMARY KEY,
    beacon_id text NOT NULL REFERENCES beacons(id) ON DELETE CASCADE,
    author_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 300),
    status text NOT NULL DEFAULT 'VISIBLE' CHECK (status IN ('VISIBLE', 'REMOVED')),
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    moderated_at timestamptz
);
CREATE INDEX beacon_comments_beacon ON beacon_comments (beacon_id, created_at DESC);
ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon','dm_message','beacon_comment'));
