-- The orders pass checks only orders that could have moved since its last run: those whose records
-- changed, and those drawn or rerolled since (docs/PROGRESSION.md).
ALTER TABLE daily_orders ADD COLUMN drawn_at timestamptz NOT NULL DEFAULT now();
