-- One dedicated game channel. The external game runner owns emulation; Core owns votes.
CREATE TABLE plays_runtime (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    channel_id text NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    game text NOT NULL CHECK (length(game) BETWEEN 1 AND 80),
    bridge_hash text NOT NULL CHECK (length(bridge_hash)=64),
    enabled boolean NOT NULL DEFAULT true,
    heartbeat_at timestamptz,
    ready boolean NOT NULL DEFAULT false,
    input_mode text NOT NULL DEFAULT 'chat' CHECK (input_mode IN ('chat','rl')),
    last_round bigint NOT NULL DEFAULT 0,
    last_command text,
    last_chosen_at timestamptz
);
CREATE TABLE plays_votes (
    round bigint NOT NULL,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    command text NOT NULL CHECK (command IN ('up','down','left','right','a','b','start','select')),
    PRIMARY KEY(round,user_id)
);
