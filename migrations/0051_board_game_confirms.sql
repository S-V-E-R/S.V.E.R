-- CrowdSync §3 "Hold, then capture" (docs/DEVELOPER_PLATFORM.md): a press on a "Game confirms"
-- control only holds the viewer's Engagement Valor until the game or bridge captures it (charged)
-- or releases it (refunded); anything still held after 60 seconds is released.
ALTER TABLE board_presses
    ADD COLUMN held_until timestamptz,
    ADD COLUMN outcome text CHECK (outcome IN ('captured', 'released'));
CREATE INDEX board_presses_held ON board_presses (held_until) WHERE held_until IS NOT NULL AND outcome IS NULL;
