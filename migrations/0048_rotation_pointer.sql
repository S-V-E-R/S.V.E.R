-- Module 5 fair rotation: the stream at the top of Live Now this slot. Each slot the pointer
-- moves to the next live stream in start order, so streams starting or ending can't skip anyone.
CREATE TABLE discovery_rotation (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    slot bigint NOT NULL,
    leader_started_at timestamptz,
    leader_id text
);
