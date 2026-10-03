-- Legacy profile export for the Module 2 import (docs/PROFILES.md, "Legacy profile import").
-- Run only against legacy.dump restored in an isolated container (no network, no published
-- ports), writing the result beside the dump, outside the workspace and Git:
--   psql -X -q -At -v ON_ERROR_STOP=1 -d legacy -f export-legacy-profiles.sql -o profiles-export.json
-- Timestamps are written as UTC with an explicit offset; everything else stays in the dump.
SET TIME ZONE 'UTC';
SELECT json_build_object(
    'Follow', (SELECT coalesce(json_agg(json_build_object(
        'followerId', "followerId", 'followingId', "followingId",
        'createdAt', to_char("createdAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"')
    ) ORDER BY id), '[]'::json) FROM "Follow"),
    'ProfileTop8', (SELECT coalesce(json_agg(json_build_object(
        'userId', "userId", 'targetUserId', "targetUserId", 'position', position
    ) ORDER BY id), '[]'::json) FROM "ProfileTop8"),
    'SocialLink', (SELECT coalesce(json_agg(json_build_object(
        'userId', "userId", 'platform', platform, 'url', url,
        'createdAt', to_char("createdAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"')
    ) ORDER BY id), '[]'::json) FROM "SocialLink"),
    'WallPost', (SELECT coalesce(json_agg(json_build_object(
        'id', id, 'authorId', "authorId", 'wallOwnerUserId', "wallOwnerUserId", 'body', body,
        'createdAt', to_char("createdAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"'),
        'deletedAt', to_char("deletedAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"'),
        'moderationStatus', "moderationStatus"::text,
        'moderatedAt', to_char("moderatedAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"')
    ) ORDER BY id), '[]'::json) FROM "WallPost"),
    'WallReply', (SELECT coalesce(json_agg(json_build_object(
        'id', id, 'postId', "postId", 'authorId', "authorId", 'body', body,
        'createdAt', to_char("createdAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"'),
        'deletedAt', to_char("deletedAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"')
    ) ORDER BY id), '[]'::json) FROM "WallReply"),
    'WallReaction', (SELECT coalesce(json_agg(json_build_object(
        'postId', "postId", 'userId', "userId", 'type', type::text,
        'createdAt', to_char("createdAt"::timestamp, 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"')
    ) ORDER BY id), '[]'::json) FROM "WallReaction")
);
