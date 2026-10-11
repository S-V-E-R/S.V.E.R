# S.V.E.R automation bridge (Streamer.bot and SAMMI)

Streamer.bot and SAMMI don't support S.V.E.R yet, so this small app on your PC fires S.V.E.R's live events into them. It signs in once with a code at `sver.tv/go` (scopes `user:read events:private`: it can read your channel's events, never move money or post), listens to your channel, and forwards each event:

| S.V.E.R event | Streamer.bot action it runs | SAMMI webhook trigger |
|---|---|---|
| You went live or offline | `S.V.E.R Live` | `sver_live` |
| Someone followed | `S.V.E.R Follow` | `sver_follow` |
| A subscription (new, renewed or gifted) | `S.V.E.R Subscription` | `sver_subscription` |
| A tribute | `S.V.E.R Tribute` | `sver_tribute` |
| A Skill was played | `S.V.E.R Skill` | `sver_skill` |
| An incoming raid | `S.V.E.R Raid` | `sver_raid` |

The event's fields arrive as arguments (Streamer.bot `%user%`, `%valor%`, `%message%` and so on; SAMMI's Trigger Pull Data), flattened to text, plus `sver_event` and `sver_at`.

## Set up

1. **Streamer.bot:** Servers/Clients → HTTP Server → turn it on (default `127.0.0.1:7474`). Create the actions above with those exact names (change the `S.V.E.R` prefix in the config if you like). Leave out any you don't need.
2. **SAMMI:** Settings → turn on the local API (default port 9450). Add buttons with a Webhook trigger whose message is the trigger name above (or `*` for all of them).
3. Copy `automation.example.json` to `automation.json`, fill in the client ID, and remove `streamerbot` or `sammi` if you use only one. Then, with Node 22 or newer:

   ```
   node sver-automation.mjs automation.json
   ```

   The first run prints a code to approve at `sver.tv/go`. The tokens stay in `automation.tokens.json` beside the config; delete it, or remove the app under Settings → Connected apps on sver.tv, to sign out.

If the connection drops, it reconnects and replays what it missed (up to 5 minutes, S.V.E.R's replay window). If Streamer.bot or SAMMI isn't running, it logs that and carries on.

## Test

`node automation/test.mjs` (from `integrations/`) checks the event mapping and a full run against a fake S.V.E.R, Streamer.bot and SAMMI. It has not yet run against real Streamer.bot or SAMMI installs.
