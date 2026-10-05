CREATE TABLE squads (
    id text PRIMARY KEY,
    host_id text REFERENCES users(id) ON DELETE SET NULL,
    mode text NOT NULL CHECK(mode IN ('SEPARATE','MERGED')),
    created_at timestamptz NOT NULL DEFAULT now(),
    ended_at timestamptz
);
CREATE TABLE squad_members (
    squad_id text NOT NULL REFERENCES squads(id) ON DELETE CASCADE,
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    joined_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX squad_roster ON squad_members(squad_id,joined_at);
CREATE TABLE squad_invites (
    id text PRIMARY KEY,
    squad_id text NOT NULL REFERENCES squads(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now()+interval '10 minutes',
    UNIQUE(squad_id,user_id)
);
CREATE INDEX squad_invitee ON squad_invites(user_id,expires_at);
CREATE TABLE squad_restrictions (
    squad_id text NOT NULL REFERENCES squads(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK(kind IN ('ban','timeout')),
    until timestamptz,
    PRIMARY KEY(squad_id,user_id,kind),
    CHECK((kind='ban' AND until IS NULL) OR (kind='timeout' AND until IS NOT NULL))
);
ALTER TABLE chat_messages ADD COLUMN squad_id text REFERENCES squads(id) ON DELETE CASCADE;
CREATE INDEX squad_chat_history ON chat_messages(squad_id,seq DESC) WHERE squad_id IS NOT NULL;
