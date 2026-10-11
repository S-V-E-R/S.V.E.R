# S.V.E.R for Stream Deck

An Elgato Stream Deck plugin (Stream Deck 7.1 or newer, Windows or macOS) with five keys:

| Key | What it does | API (docs/DEVELOPER_PLATFORM.md "App actions") |
|---|---|---|
| Set stream title | Changes the title to the one you set; the category stays | `PATCH /api/me/stream` (`channel:edit`) |
| Raid | Raids the channel you set | `POST /api/me/raids` (`channel:run`) |
| Start poll or prediction | Starts the question and options you set (15–1800 seconds) | `POST /api/channels/{you}/polls` |
| Mark highlight | Adds a highlight marker to your live stream | `POST /api/channels/{you}/marker` |
| Pause board | Panic switch: pauses or resumes your board; the key shows which | `PUT /api/channels/{you}/board/disabled` |

A check mark means it worked; the warning triangle means it didn't, and the key's settings panel says why (for example "That channel isn't live.").

## Sign in

Open any S.V.E.R key's settings and choose **Sign in to S.V.E.R**. The panel shows a code and opens `sver.tv/go`, where you approve it (device sign-in). The plugin asks only for `user:read channel:edit channel:run`: it can never move money or read private messages. Its tokens stay in Stream Deck's settings on this PC; **Sign out**, or remove "S.V.E.R for Stream Deck" under Settings → Connected apps on sver.tv, ends it at once.

## Install (development)

Copy or link `tv.sver.streamdeck.sdPlugin` into Stream Deck's plugins folder (`%appdata%\Elgato\StreamDeck\Plugins` on Windows, `~/Library/Application Support/com.elgato.StreamDeck/Plugins` on macOS) and restart Stream Deck. No `npm install`: it uses Stream Deck's bundled Node 24.

`CLIENT_ID` in `bin/sver.mjs` must be the official S.V.E.R Stream Deck app's client ID from Settings → Developer (public, not a secret).

## Test

`node streamdeck/test.mjs` (from `integrations/`) checks sign-in, token refresh, every action's request and the plugin's Stream Deck messages against fakes. It has not yet run on a real Stream Deck.
