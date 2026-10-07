-- Plays health (docs/PLAYS.md, reliability): the host watchdog's last reported problem, and when
-- staff were last alerted, so an outage emails admins at most once an hour.
ALTER TABLE plays_runtime
    ADD COLUMN problem text CHECK (length(problem) <= 200),
    ADD COLUMN problem_since timestamptz,
    ADD COLUMN alerted_at timestamptz;
