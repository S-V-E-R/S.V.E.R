-- Progression part 3 (docs/PROGRESSION.md section 3): daily orders. Progress is read from the
-- event records each order type names; completion is stamped by the minute tick and pays XP once.
ALTER TABLE xp_days ADD COLUMN minutes integer NOT NULL DEFAULT 0;
CREATE TABLE daily_orders (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day date NOT NULL,
    slot smallint NOT NULL CHECK (slot BETWEEN 0 AND 2),
    kind text NOT NULL,
    rarity smallint NOT NULL CHECK (rarity BETWEEN 0 AND 4),
    target integer NOT NULL CHECK (target > 0),
    rerolled boolean NOT NULL DEFAULT false,
    completed_at timestamptz,
    xp integer,
    PRIMARY KEY (user_id, day, slot)
);
CREATE INDEX daily_orders_open ON daily_orders (day) WHERE completed_at IS NULL;
-- Weekly milestones (10, 20 and 30 orders in an ISO week), paid once each.
CREATE TABLE order_milestones (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    week date NOT NULL,
    orders smallint NOT NULL,
    PRIMARY KEY (user_id, week, orders)
);
