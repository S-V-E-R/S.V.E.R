-- Outside (linked-chat) messages can be reported to staff (docs/LINKED_CHAT.md "Moderation").
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon','dm_message','beacon_comment','wall_tile','light','outside_message'));
