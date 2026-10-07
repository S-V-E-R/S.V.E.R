-- Module 5 MAGNet chat: a feature whose chat was merged with the lane's room. Its window's
-- channel messages stay in the room's history after the switch (docs/MAGNET.md, MAGNet chat).
ALTER TABLE magnet_features ADD COLUMN merged boolean NOT NULL DEFAULT false;
