-- MAGNet game signals (docs/MAGNET.md "What counts as a moment"): a win the game detector saw on screen, kept a
-- day, so a stream that just won can be featured as a moment whether or not it suggests a clip.
CREATE TABLE game_moments (
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    title text NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX game_moments_recent ON game_moments (broadcast_id, at);

-- CrowdSync activity in a channel since a time, one row per action: free board presses and poll
-- votes. Paid presses and predictions never count, so a moment can't be bought.
CREATE FUNCTION magnet_crowd(channel text, since timestamptz) RETURNS TABLE(u text, at timestamptz) AS $$
    SELECT p.user_id, p.created_at FROM board_presses p WHERE p.channel_id=channel AND p.cost=0 AND p.created_at>since
    UNION ALL
    SELECT v.user_id, v.created_at FROM poll_votes v JOIN polls q ON q.id=v.poll_id
    WHERE q.channel_id=channel AND q.kind='poll' AND v.created_at>since
$$ LANGUAGE sql STABLE;
