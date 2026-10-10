-- Developer platform §7 (docs/DEVELOPER_PLATFORM.md "BTTV, 7TV and FrankerFaceZ emotes"): a streamer
-- who linked Twitch can show their 7TV, BTTV and FFZ channel emotes (and, optionally, global sets)
-- in S.V.E.R chat. Lists are read every 10 minutes; images are copied to S.V.E.R's media storage so
-- viewers never contact those services. Streamers hide any emote; a viewer report hides one until
-- the streamer reviews it.
ALTER TABLE chat_settings
    ADD COLUMN outside_emotes text[] NOT NULL DEFAULT '{}' CHECK (outside_emotes <@ ARRAY['7tv','bttv','ffz']),
    ADD COLUMN outside_global boolean NOT NULL DEFAULT false,
    ADD COLUMN outside_synced_at timestamptz;

-- A channel's emotes from each service; channel_id NULL is that service's global set.
CREATE TABLE outside_emotes (
    channel_id text REFERENCES users(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (provider IN ('7tv','bttv','ffz')),
    emote_id text NOT NULL CHECK (emote_id ~ '^[A-Za-z0-9]{1,40}$'),
    code text NOT NULL CHECK (char_length(code) BETWEEN 1 AND 40),
    animated boolean NOT NULL DEFAULT false,
    seen_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX outside_emotes_unique ON outside_emotes (coalesce(channel_id,''), provider, emote_id);

-- Images copied to S.V.E.R's storage, once per emote (shared by every channel showing it).
CREATE TABLE outside_emote_files (
    provider text NOT NULL,
    emote_id text NOT NULL,
    ext text NOT NULL CHECK (ext IN ('webp','png','gif')),
    stored_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, emote_id)
);

CREATE TABLE outside_emote_hides (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider text NOT NULL,
    emote_id text NOT NULL,
    reason text NOT NULL CHECK (reason IN ('streamer','report')),
    reported_by text REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, provider, emote_id)
);
