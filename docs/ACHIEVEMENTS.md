# Achievements and cosmetics

Phase 3. Joe chose milestones and cosmetics on October 10, 2026 (not legacy's staff-run achievement hunts). Numbers are **Proposed** defaults he can change. Builds on [PROGRESSION.md](PROGRESSION.md) (levels, the badge frame every 10 levels).

## Achievements

Earned automatically from records S.V.E.R already keeps, checked every 10 minutes, never from what a browser reports. Once earned, kept. Shown on the profile with the date earned.

| Key | Name | Earned when |
| --- | --- | --- |
| `first_follow` | First Follow | You followed a channel |
| `voice` | Voice of the Crowd | You chatted in 10 different channels |
| `scout` | Scout | You scouted a new stream |
| `pathfinder` | Pathfinder | You scouted 10 new streams |
| `order_keeper` | Order Keeper | 30 daily orders done in one week |
| `legend` | Legend | Legend loyalty rank in a channel |
| `level_10`, `level_25`, `level_50`, `level_100` | Level 10 … Level 100 | Reached that account level |
| `first_light` | First Light | You went live for the first time |
| `hours_10`, `hours_100` | Ten Hours Lit, Hundred Hours Lit | Streamed that many hours |
| `followers_100`, `followers_1000` | Gathering, Host of a Thousand | Reached that many followers |
| `raider` | Raider | You led a raid that moved |
| `supported` | Supported | Your first subscriber |
| `first_tribute` | Patron | You paid a first tribute |
| `faithful` | Faithful | You subscribed to a channel for 6 months |
| `signatory` | Signatory | You signed someone's Wall (approved) |
| `council_of_war` | Council of War | You filled all 8 War Council seats |

## Cosmetics (Settings → Cosmetics)

- **Name style in chat**, always in your faction's colors: Plain (everyone), Bold (level 10), Small caps (level 20), Accent underline (level 30), Outlined (level 50).
- **Badge frame:** any frame you've unlocked (a new one every 10 levels); the newest is used until you choose.
- **Showcase:** up to 3 of your achievements on your profile and your user card.

Cosmetics never affect MAGNet, rotation, influence or money.

## Done when

1. Each achievement is earned exactly once, by doing the thing, and never from a client report.
2. A style or frame above your level, or an achievement you don't have, can't be chosen.
3. The chosen style shows in chat, the frame on the player and user cards, the showcase on the profile and user card.

## As built (October 10, 2026; `achievements.rs`, migration 0085)

Covered by `tests/streams/achievements.rs`. Since October 17, 2026, each pass skips people who already hold a key, and the four level achievements come from one pass over `xp_days`.
