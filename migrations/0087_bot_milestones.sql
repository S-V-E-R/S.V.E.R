-- Bot lines for Surge levels and follower milestones (docs/COMMUNITY.md "Chat commands and the channel bot").
ALTER TABLE bot_events DROP CONSTRAINT bot_events_kind_check;
ALTER TABLE bot_events ADD CONSTRAINT bot_events_kind_check CHECK (kind IN ('follow', 'sub', 'raid', 'surge', 'milestone'));
CREATE TABLE bot_milestones (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    followers integer NOT NULL,
    PRIMARY KEY (channel_id, followers)
);
