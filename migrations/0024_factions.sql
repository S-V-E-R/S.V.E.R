-- Module 4 owns membership, the genre war and its immutable contribution history.
CREATE TABLE faction_members (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL CHECK (faction IN ('myria','aetheron','glint')),
    chosen_at timestamptz NOT NULL,
    joined_at timestamptz NOT NULL,
    free_switch_used boolean NOT NULL DEFAULT false,
    moderator_candidate boolean NOT NULL DEFAULT false
);
CREATE INDEX faction_member_directory ON faction_members(faction,joined_at,user_id);
CREATE TABLE faction_switches (
    id bigserial PRIMARY KEY,
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    from_faction text,
    to_faction text NOT NULL,
    happened_at timestamptz NOT NULL,
    reason text NOT NULL CHECK (reason IN ('join','free_switch','season_break','legacy_import'))
);

CREATE TABLE faction_genres (
    id text PRIMARY KEY,
    name text NOT NULL UNIQUE,
    home text CHECK (home IN ('myria','aetheron','glint')),
    position integer NOT NULL,
    neighbors text[] NOT NULL DEFAULT '{}'
);
INSERT INTO faction_genres(id,name,home,position) VALUES
 ('fps_battle_royale','FPS & battle royale','myria',0),
 ('fighting','Fighting','myria',1),
 ('sports_racing','Sports & racing','myria',2),
 ('speedrunning','Speedrunning','myria',3),
 ('crafting_making','Crafting & making','myria',4),
 ('rts_moba','RTS & MOBA','aetheron',5),
 ('strategy_4x','Strategy & 4X','aetheron',6),
 ('card_board','Card & board','aetheron',7),
 ('puzzle_simulation','Puzzle & simulation','aetheron',8),
 ('art','Art','aetheron',9),
 ('education_coding','Education & coding','aetheron',10),
 ('community_events','Community events','glint',11),
 ('mmos_rpgs','MMOs & RPGs','glint',12),
 ('coop_party','Co-op & party','glint',13),
 ('cozy_sandbox','Cozy & sandbox','glint',14),
 ('music','Music','glint',15);
UPDATE faction_genres g SET neighbors=ARRAY(SELECT n.id FROM faction_genres n WHERE abs(n.position-g.position)=1 ORDER BY n.position);
-- Consolidate existing category aliases into the approved genre groups.
UPDATE stream_categories SET genre=CASE genre
 WHEN 'fps' THEN 'fps_battle_royale' WHEN 'battle_royale' THEN 'fps_battle_royale'
 WHEN 'rts' THEN 'rts_moba' WHEN 'moba' THEN 'rts_moba'
 WHEN 'sandbox' THEN 'cozy_sandbox' ELSE genre END;
INSERT INTO faction_genres(id,name,position)
 SELECT genre,initcap(replace(genre,'_',' ')),100+row_number() OVER (ORDER BY genre)
 FROM (SELECT DISTINCT genre FROM stream_categories) c
 WHERE NOT EXISTS(SELECT 1 FROM faction_genres g WHERE g.id=c.genre);
ALTER TABLE stream_categories ADD CONSTRAINT category_genre FOREIGN KEY(genre) REFERENCES faction_genres(id);

CREATE TABLE faction_seasons (
    id bigserial PRIMARY KEY,
    number integer NOT NULL UNIQUE CHECK(number>0),
    starts_at timestamptz NOT NULL UNIQUE,
    ends_at timestamptz NOT NULL,
    next_starts_at timestamptz NOT NULL,
    finished_at timestamptz,
    winners text[] NOT NULL DEFAULT '{}',
    CHECK(starts_at<ends_at AND ends_at<next_starts_at)
);
CREATE TABLE faction_weeks (
    id bigserial PRIMARY KEY,
    season_id bigint NOT NULL REFERENCES faction_seasons(id),
    starts_at timestamptz NOT NULL UNIQUE,
    ends_at timestamptz NOT NULL,
    completed_at timestamptz,
    attempts integer NOT NULL DEFAULT 0,
    last_error text,
    result jsonb,
    CHECK(starts_at<ends_at)
);
CREATE INDEX faction_checkpoints_due ON faction_weeks(ends_at) WHERE completed_at IS NULL;
CREATE TABLE faction_territories (
    season_id bigint NOT NULL REFERENCES faction_seasons(id),
    genre text NOT NULL REFERENCES faction_genres(id),
    holder text CHECK(holder IN ('myria','aetheron','glint')),
    PRIMARY KEY(season_id,genre)
);
CREATE TABLE faction_influence (
    id bigserial PRIMARY KEY,
    event_key text NOT NULL UNIQUE,
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    faction text NOT NULL CHECK(faction IN ('myria','aetheron','glint')),
    week_id bigint NOT NULL REFERENCES faction_weeks(id),
    genre text NOT NULL REFERENCES faction_genres(id),
    source text NOT NULL CHECK(source IN ('stream','watch','chat','support')),
    points bigint NOT NULL CHECK(points>0),
    happened_at timestamptz NOT NULL
);
CREATE INDEX faction_influence_week ON faction_influence(week_id,genre,faction);
CREATE INDEX faction_influence_cap ON faction_influence(user_id,source,happened_at);
CREATE INDEX faction_influence_active ON faction_influence(happened_at,faction,user_id);
-- A time cursor prevents overlapping tabs/channels from multiplying a person's elapsed time.
CREATE TABLE faction_time_credit (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source text NOT NULL CHECK(source IN ('stream','watch')),
    last_at timestamptz NOT NULL,
    PRIMARY KEY(user_id,source)
);
-- Account erasure removes attribution, never historical points or replay protection.
CREATE FUNCTION protect_faction_influence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.user_id IS NULL AND OLD.user_id IS NOT NULL
    AND (to_jsonb(NEW)-'user_id')=(to_jsonb(OLD)-'user_id') THEN RETURN NEW; END IF;
 RAISE EXCEPTION 'Faction influence is append-only';
END $$;
CREATE TRIGGER faction_influence_append_only BEFORE UPDATE OR DELETE ON faction_influence
 FOR EACH ROW EXECUTE FUNCTION protect_faction_influence();

CREATE TABLE faction_votes (
    week_id bigint NOT NULL REFERENCES faction_weeks(id),
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL,
    genre text NOT NULL REFERENCES faction_genres(id),
    PRIMARY KEY(week_id,user_id)
);
CREATE TABLE faction_targets (
    week_id bigint NOT NULL REFERENCES faction_weeks(id),
    faction text NOT NULL,
    genre text NOT NULL REFERENCES faction_genres(id),
    PRIMARY KEY(week_id,faction)
);
CREATE TABLE faction_moderator_votes (
    week_id bigint NOT NULL REFERENCES faction_weeks(id),
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    candidate_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL,
    PRIMARY KEY(week_id,user_id)
);
CREATE TABLE faction_moderators (
    week_id bigint NOT NULL REFERENCES faction_weeks(id),
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL,
    PRIMARY KEY(week_id,user_id)
);
CREATE TABLE faction_posts (
    id text PRIMARY KEY,
    faction text NOT NULL CHECK(faction IN ('myria','aetheron','glint')),
    author_id text REFERENCES users(id) ON DELETE SET NULL,
    body text NOT NULL CHECK(char_length(body) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    status text NOT NULL DEFAULT 'VISIBLE' CHECK(status IN ('VISIBLE','REMOVED','DELETED'))
);
CREATE INDEX faction_board_page ON faction_posts(faction,created_at DESC,id DESC);
CREATE INDEX faction_board_slow ON faction_posts(author_id,created_at DESC);
CREATE TABLE faction_rewards (
    season_id bigint NOT NULL REFERENCES faction_seasons(id),
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL,
    awarded_at timestamptz NOT NULL,
    -- Module 6 will settle the entitlement through its Valor ledger, once only.
    valor_pending boolean NOT NULL DEFAULT true,
    PRIMARY KEY(season_id,user_id)
);
ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK(target_type IN
 ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post'));
CREATE TABLE faction_support_events (
    event_key text PRIMARY KEY,
    pair_key text NOT NULL
);
CREATE INDEX faction_support_pair ON faction_support_events(pair_key);
