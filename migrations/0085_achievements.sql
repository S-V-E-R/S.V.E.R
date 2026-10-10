-- Achievements and cosmetics (docs/ACHIEVEMENTS.md).
CREATE TABLE user_achievements (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key text NOT NULL,
    earned_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);
CREATE TABLE user_cosmetics (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    name_style text NOT NULL DEFAULT 'plain' CHECK (name_style IN ('plain', 'bold', 'smallcaps', 'accent', 'outlined')),
    frame smallint CHECK (frame BETWEEN 0 AND 10),
    showcase text[] NOT NULL DEFAULT '{}' CHECK (cardinality(showcase) <= 3)
);
