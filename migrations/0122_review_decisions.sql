-- Joe's decisions after the touch-packet 6 review (October 17, 2026).

-- Charity links: a new link shows its Donate banner only after staff approve it once; approved
-- links then work at once for anyone who uses them. Links already set wait for that first check.
CREATE TABLE charity_links (
    url text PRIMARY KEY,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    charity_name text NOT NULL,
    requested_by text REFERENCES users(id) ON DELETE SET NULL,
    requested_at timestamptz NOT NULL DEFAULT now(),
    reviewed_by text REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    note text
);
CREATE INDEX charity_links_pending ON charity_links (requested_at) WHERE status = 'pending';
INSERT INTO charity_links (url, charity_name, requested_by)
SELECT DISTINCT ON (charity_url) charity_url, charity_name, owner_id
FROM stream_settings
WHERE charity_url IS NOT NULL AND charity_name IS NOT NULL
ORDER BY charity_url, updated_at DESC
ON CONFLICT DO NOTHING;

-- Lights stay public, and their messages can be reported; staff removal is kept apart from the
-- recipient hiding a light, so an appeal restores exactly what staff removed.
ALTER TABLE lights ADD COLUMN removed_at timestamptz;
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon','dm_message','beacon_comment','wall_tile','light'));
