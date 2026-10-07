# S.V.E.R Plays

Expanded October 3, 2026 by Joe. S.V.E.R Plays is S.V.E.R's own always-on channel where chat plays a game together, and an AI player keeps it going when nobody is watching. It stays its own project (the runner lives in the `S-V-E-R/Plays` repo) and talks to the platform only through the internal interface in [LIVE_STREAMS.md](LIVE_STREAMS.md#sver-plays). This file is the product spec for both sides.

## What it is

- A game runs in an emulator on the server, and its video is sent to S.V.E.R like any other stream.
- Chat controls it: viewers type commands (`up`, `a`, `start` …) or use the vote panel under the player.
- When no verified viewer is watching, the AI player takes over so the game never stops. As soon as a real viewer is counted, control goes back to chat.
- It is also the stream S.V.E.R uses to monitor live delivery and viewer integrity, because it is always on.

## Games

- **The game cycles**, like the games any streamer plays. Pokémon Yellow is the current game. When a game is finished, or on a schedule, the community votes on the next game from the Plays library (see "Next game" below).
- Each game is an adapter in the Plays runner: controls, a save state, and the game events it can read (for milestones).
- The runner keeps one shared community save per game, written every 60 seconds and on stop, and restores it after a restart.
- Ownership note: every game in the library is run from a legally owned copy, and S.V.E.R never distributes game files, saves or AI models. Before rewards attach to a game, a lawyer reviews running it as S.V.E.R's own channel.

## When it's built

| Step | When | What |
| --- | --- | --- |
| 1. Move | Right after Live streams closes | Plays runs on the new API: integrity-based control gate, counted-session votes, Democracy and Anarchy, stream-time vote windows, the controller board (the first CrowdSync board), reliability fixes |
| 2. Fill empty moments | With MAGNet (Module 5) | Plays appears on the homepage and MAGNet channels when nothing else is live |
| 3. Faction credit | With Factions (Module 4) | Milestones credited to the faction whose votes drove them |
| 4. Rewards and milestones | With Support (Module 6) | Capped Engagement Valor for voting; milestones and badges from real game events |
| 5. Next game vote | After step 4 | The community picks the next game from the library |
| With CrowdSync (Module 7) | | CrowdSync generalizes the Plays board so every streamer can build boards |

## Who can control the game

Decided earlier, in [LIVE_STREAMS.md](LIVE_STREAMS.md#sver-plays):
- The AI player plays only after no signed-in, verified viewer has been counted for the grace period (60 seconds). Missing or stale counts mean chat mode: humans always win.
- A vote counts only if the voter has a Counted or Trusted viewing session on the Plays broadcast. Chat-only accounts, guests and excluded sessions can't vote.

## Input modes

- **Democracy (default):** each window, every eligible viewer gets one vote; the most-voted command runs; ties are random; an empty window runs nothing. Window length scales with the number of counted voters (3 to 6 seconds, as in legacy).
- **Anarchy:** every eligible command runs in order, limited to one command per viewer per second and a total of 10 commands per second (**Proposed**).
- **Switching modes:** chat votes on the mode. A switch needs a clear majority of counted viewers (**Proposed:** 75% of votes in a 30-second vote, at most once every 5 minutes), so a few accounts can't flip it.
- **Fair for delayed viewers:** vote windows run on stream time. A viewer 3–5 seconds behind gets the same window as a viewer 1 second behind, and votes are matched to the window the viewer was actually watching.
- The command list is per game; `select` and other disruptive commands can be limited in Anarchy.

## The controller board (decided October 3, 2026)

Plays is the **first CrowdSync board**. It is built in step 1 using CrowdSync's board format ([CROWDSYNC.md](CROWDSYNC.md)), so it proves the board design early; Module 7 later opens board building to every streamer. The design is on the S.V.E.R design canvas ("Watch pages: S.V.E.R Plays").

- **Placement:** directly under the player, above the streamer bar's details; chat stays on the right. On phones it stacks under the player.
- **Controller:** laid out like a game controller: D-pad on the left, Select and Start in the middle, B and A on the right. The buttons come from the current game's command list. Each button shows its live share of the current window's votes as a percentage and a fill; the leading button is outlined; the viewer's own vote is highlighted. Real buttons, keyboard reachable, with labels for screen readers.
- **Control status bar:** the mode (Democracy or Anarchy), who is in control ("Chat is in control" or "AI is playing · vote to take over"), a countdown to the next input in stream time with a progress bar, and the viewer's current vote.
- **Input history strip:** the last 10 inputs that ran, each marked with the faction color of the voters who won it.
- **Faction tug-of-war:** each faction's share of winning votes this hour.
- **Milestone tracker:** progress to the next milestone (for example "Badge 3 of 8") and the last milestone with its faction credit.
- **Mode switch:** a vote bar showing progress toward the 75% needed, with a Vote button and the time left.
- **Chat:** typed commands still count as votes and show as small command chips; the window results and milestones appear as quiet system lines.
- Guests see the board but get a sign-in prompt instead of voting; viewers without a counted session see why they can't vote yet.

## Rewards and milestones (step 4)

- **Voting earns the Plays channel's Engagement Valor**, capped per account per day (**Proposed:** 200 a day). Only Trusted sessions earn.
- **Milestones come from the real game**, read by the game's adapter: badges, captures, story beats, levels, finishing the game. The legacy "expedition" meters, which never read the game, are dropped.
- Each milestone appears on stream and in a Plays milestones page with the date, and everyone whose votes counted in the hour before it gets a Plays badge for that milestone.
- No Purchased Valor or money is involved.

## Faction credit (step 3)

- When a milestone happens, the faction with the most counted votes in the windows leading to it gets the credit.
- A Plays faction leaderboard shows milestones per faction for each game and season.
- Faction influence from Plays counts only as ordinary chat participation, under its existing cap. The legacy +50 influence per window is dropped.

## Filling empty moments (step 2)

- When no creator is live, the homepage's live row and every MAGNet channel show Plays instead of an empty view, labeled "S.V.E.R Plays: no one's live right now, play along".
- As soon as any creator is live and eligible, they take priority (Plays is featured only when nothing else is live, per [MAGNET.md](MAGNET.md)).

## Next game (step 5)

- When the current game is finished, or at a scheduled rotation, chat votes on the next game from the library (3 to 5 choices shown on stream).
- The vote uses the same counted-viewer rules. The current game's save is kept, so it can come back later.

## Reliability fixes (step 1)

Status, October 7, 2026: crash restart, Postgres vote state, the channel setting, no recording and co-streams were already in place. The stream now runs at real time: the adapter had emitted in-between frames faster than real time and the runner padded on top, so about 3 seconds of video went out per second and HLS viewers stalled. Every frame now takes one wall-clock slot, and ffmpeg stamps frames with the wall clock at a constant rate as a backstop. Alerts: `infra/plays/watchdog.py` runs every minute beside the game, restarts it after two checks without a moving picture (at most once every five minutes), and reports the problem through the bridge; Core emails staff admins when a problem, a silent bridge or an offline stream lasts more than two minutes, at most once an hour (migration `0047`). Still open: game audio. Writing audio per emulator frame is needed first, and a starved audio pipe would stall the whole stream, so it ships separately once it can't block video.

- The runner's start script currently keeps reporting "running" when the emulator crashes, because it waits on the endless video loop. It must exit and restart when either process stops.
- Vote state moves from one in-memory process to Postgres, so a restart doesn't lose votes or cooldowns.
- Alerts for a frozen picture, a stalled game or a stopped stream (Plays is the monitoring stream, so its own health must be watched).
- Smoother picture (more frequent input steps) and game audio, once their timing is verified.
- The username `sverplays` is no longer hardcoded; the Plays channel is a setting.
- Recording: the 24/7 Plays channel is never recorded or clipped (Module 8). Its segments are acknowledged and discarded, whatever its channel settings say, so it cannot fill recording storage.
- Co-streams: nobody signs in as the Plays channel, so it joins a co-stream as soon as it's invited, but only by the one streamer set in `plays_runtime.costream_host_id` (a user ID, set by an operator; migration `0041`). Invitations from anyone else stay pending and expire. Use separate chat: in a shared chat, game votes from chat don't count.

## Done when (step 1)

On the new API, Plays streams continuously; bots, guests and chat-only accounts can't vote or keep the AI off; a real viewer takes control back within about a minute; Democracy and Anarchy work and the mode switch needs a clear majority; the controller board shows live vote shares, the control status, input history, tug-of-war and milestones; votes from 1-second and 5-second viewers land in the right windows; a crash restarts the stream automatically and an alert fires on a frozen picture.
