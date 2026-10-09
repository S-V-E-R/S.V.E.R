-- Progression part 2 (docs/PROGRESSION.md section 5): the chat rank gate. 0 is off; 1-4 require
-- Regular, Devoted, Veteran or Legend loyalty (lifetime Engagement Valor earned in the channel).
ALTER TABLE chat_settings ADD COLUMN min_loyalty smallint NOT NULL DEFAULT 0 CHECK (min_loyalty BETWEEN 0 AND 4);
