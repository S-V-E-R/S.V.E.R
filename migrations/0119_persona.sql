-- Persona pass (docs/PROFILES.md "Persona pass", Joe, October 16, 2026).
ALTER TABLE profiles
    ADD COLUMN currently_verb text NOT NULL DEFAULT ''
        CHECK (currently_verb IN ('', 'PLAYING', 'LISTENING', 'WATCHING', 'READING', 'BUILDING', 'LEARNING')),
    ADD COLUMN currently_text text NOT NULL DEFAULT '' CHECK (char_length(currently_text) <= 60),
    ADD COLUMN song_why text NOT NULL DEFAULT '' CHECK (char_length(song_why) <= 80),
    -- '' means the faction's default variant.
    ADD COLUMN theme_variant text NOT NULL DEFAULT ''
        CHECK (theme_variant IN ('', 'forge', 'ash', 'moon', 'deep', 'crown', 'dawn')),
    -- '' means automatic: stream first for anyone who has streamed, persona first otherwise.
    ADD COLUMN page_layout text NOT NULL DEFAULT '' CHECK (page_layout IN ('', 'stream', 'persona')),
    ADD COLUMN home_order text[] NOT NULL DEFAULT '{}';

ALTER TABLE profile_blocks DROP CONSTRAINT profile_blocks_type_check;
ALTER TABLE profile_blocks ADD CONSTRAINT profile_blocks_type_check
    CHECK (type IN ('ABOUT', 'PANEL', 'QUOTES', 'GAME_SHELF', 'PROMPT'));

-- One optional sticker from a fixed set on a Wall signature.
ALTER TABLE wall_posts ADD COLUMN sticker text
    CHECK (sticker IN ('torch', 'crown', 'moon', 'flame', 'star', 'heart', 'gg', 'wave'));
