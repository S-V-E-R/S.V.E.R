-- The Wall (docs/WALL.md): 1,000 cells on /wall sold once as seats (1x1), tables (2x2) and halls
-- (4x4). A seat buys its tile and nothing else.
CREATE TABLE wall_settings (
    id boolean PRIMARY KEY DEFAULT true CHECK (id),
    -- Cents per size for each price stage, and the cells sold at which each later stage starts.
    prices jsonb NOT NULL,
    stages integer[] NOT NULL,
    limits jsonb NOT NULL,
    -- NULL until staff open sales, after the Wall terms, tax and prices are settled.
    sales_opened_at timestamptz
);
INSERT INTO wall_settings(prices, stages, limits) VALUES (
    '{"seat":[2500,3000,3500],"table":[9000,11000,12500],"hall":[30000,36000,42000]}',
    '{250,600}',
    '{"table":60,"hall":20}'
);

CREATE TABLE wall_tiles (
    n bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    x integer NOT NULL CHECK (x BETWEEN 0 AND 39),
    y integer NOT NULL CHECK (y BETWEEN 0 AND 24),
    size integer NOT NULL CHECK (size IN (1, 2, 4)),
    -- held: unpaid, cells reserved until hold_until. expired: the hold lapsed unpaid.
    -- in_review: paid, waiting for staff. live: approved. rejected: paid, can resubmit for 30 days.
    -- refund_due: a refund is owed and the cells are free. refunded. removed: taken down, spot stays empty.
    status text NOT NULL DEFAULT 'held' CHECK (status IN ('held','expired','in_review','live','rejected','refund_due','refunded','removed')),
    price_cents integer,
    stage integer,
    image_key text,
    name text,
    link text,
    message text,
    crest text,
    -- A change to a live tile waiting for review; the approved version stays up meanwhile.
    pending jsonb,
    review_note text NOT NULL DEFAULT '',
    hold_until timestamptz NOT NULL,
    checkout_id text,
    payment_intent text,
    paid_at timestamptz,
    rejected_at timestamptz,
    approved_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX wall_tiles_owner ON wall_tiles(owner_id, created_at DESC);
CREATE INDEX wall_tiles_review ON wall_tiles(updated_at) WHERE status = 'in_review' OR pending IS NOT NULL;

-- One row per occupied cell: a hold inserts all of its cells in one transaction, so two buyers
-- can never get overlapping spots.
CREATE TABLE wall_cells (
    x integer NOT NULL,
    y integer NOT NULL,
    tile_n bigint NOT NULL REFERENCES wall_tiles(n) ON DELETE CASCADE,
    PRIMARY KEY (x, y)
);
CREATE INDEX wall_cells_tile ON wall_cells(tile_n);

ALTER TABLE checkout_sessions DROP CONSTRAINT checkout_sessions_kind_check;
ALTER TABLE checkout_sessions ADD CONSTRAINT checkout_sessions_kind_check CHECK (kind IN ('valor', 'sub', 'gift', 'wall'));

ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon','dm_message','beacon_comment','wall_tile'));
