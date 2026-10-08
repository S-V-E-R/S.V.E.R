-- CrowdSync §3 "Groups" (docs/DEVELOPER_PLATFORM.md): a game splits viewers into groups (by
-- faction, a random split or a list of viewers), each shown one screen of the board. Resolved form:
-- {"by": "faction", "map": {"myria": 0, …}} | {"by": "random", "map": [0, 1]} |
-- {"by": "users", "map": {"<user id>": 1, …}}. Publishing clears it.
ALTER TABLE boards ADD COLUMN groups jsonb NOT NULL DEFAULT '{}';
