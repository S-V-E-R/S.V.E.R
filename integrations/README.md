# S.V.E.R integrations

Connect your stream's board ([docs/CROWDSYNC.md](../docs/CROWDSYNC.md)) to OBS and to games. Everything here talks only to S.V.E.R's integration gateway with a scoped token. Nothing connects to S.V.E.R's database or internal services.

| Folder | What it is | Status |
|---|---|---|
| [`bridge/`](bridge) | The S.V.E.R bridge: board presses switch OBS scenes, sources and filters | Tested (`node bridge/test.mjs`) |
| [`sdk-js/`](sdk-js) | JavaScript/TypeScript SDK for browser and Node games | Tested (`node sdk-js/test.mjs`) |
| [`example-game/`](example-game) | Crowd Runner, a small browser game built on the JS SDK | Runs in any browser |
| [`sdk-unity/`](sdk-unity) | Unity package (`tv.sver.board`) | Compiled in Unity 6.6; run end to end against a local API (October 15, 2026) |
| [`example-unity/`](example-unity) | Crowd Runner for Unity, on the Unity package | Builds a Windows player: `Unity -batchmode -quit -projectPath integrations/example-unity -executeMethod Build.Windows`; run with `-sverToken <token> [-sverGateway <url>]` |
| [`sdk-unreal/`](sdk-unreal) | Unreal Engine 5 plugin (`SverBoard`) | Compiled with UE 5.8; run end to end against a local API (October 15, 2026) |
| [`example-unreal/`](example-unreal) | Crowd Runner for Unreal, on the plugin: an actor for levels and a headless commandlet | `UnrealEditor-Cmd CrowdRunner.uproject -run=CrowdRunner -token=<token> [-gateway=<url>] [-seconds=60]` |
| [`streamdeck/`](streamdeck) | Stream Deck plugin: title, raid, polls and predictions, highlight markers, board pause | Tested against fakes (`node streamdeck/test.mjs`); not yet run on a real Stream Deck |
| [`automation/`](automation) | Automation bridge: S.V.E.R events into Streamer.bot (DoAction) and SAMMI (webhook triggers) | Tested against fakes (`node automation/test.mjs`); not yet run against real Streamer.bot or SAMMI |

## Get a token

Two ways for a game to connect:

- **Connect to S.V.E.R (sign-in).** The developer registers the game once in Settings → Developer and puts its client ID in the game. It is a public ID: there is no client secret, and none belongs in a game. The SDK shows the streamer a 6-character code to enter at `sver.tv/go`, where they approve the game and may tick **May set my board**. The SDK keeps that sign-in and renews it, so the code is asked for once; the streamer removes the game in Settings → Connected apps, and the SDK then shows a new code on the next connect.
- **A token from Creator Studio.** Creator Studio → Board → Connections → Create token. Choose **OBS bridge** or **Game**. For a game, tick **May set my board** to let it bring its own board (off unless you tick it; change it any time). The token is shown once; keep it out of source control. Disconnect it there any time: new connections stop at once and open ones close within 30 seconds.

## The gateway protocol

Connect a WebSocket to `wss://sver.tv/api/integrations/ws` with the token as `Authorization: Bearer <token>`, or as `?token=<token>` where headers aren't possible (browsers). Messages are JSON text frames. An app signed in through OAuth can connect with its access token instead, if it has the `board:control` scope: send its client ID as `SVER-Client-Id` (or `?client_id=`). It connects as a game; S.V.E.R Desktop connects as the bridge.

From S.V.E.R:

| `type` | When | Fields |
|---|---|---|
| `hello` | On connect | `kind` (`bridge`/`game`), `channel`, `protocol` (1), `board` (the published board, or null), `version`, `disabled` (paused), `state`, `goals`, `starting` (a game is connected but hasn't said `ready`), `input_cap` |
| `board_effect` | A viewer pressed a control | `id` (the press), `confirm` (true when it waits for your `capture`/`release`), `control`, `label`, `effect`, `user.username`, `text` (text inputs), `goal` (`progress`, `target`, `reached`), `stream_ms`, `at` |
| `board_effect` without `control` | A Skill, emote combo or Surge level played | `skill` or `surge`, `effect`, `caption` |
| `board_input` | A viewer moved a joystick | `control`, `x`, `y` (−1 to 1), `user.username` |
| `board` | A new version went live (the streamer published, a game defined its board, or the streamer's board came back), or the board was paused or resumed | The same snapshot fields as `hello` |
| `board_state` | A game changed labels, availability or goal progress | `state`, `goals` |
| `board_result` | A "Game confirms" press was captured or released (including the automatic release after 60 seconds) | `id` (the press), `control`, `outcome` (`captured`/`released`) |
| `ack` / `error` / `pong` | Replies to your messages | `id` (yours), `message` (errors), `version` (the `ack` of a `define`) |

To S.V.E.R (at most 10 messages a second):

- `{"type": "ping", "id": 1}`
- `{"type": "state", "id": 2, "controls": {"jump": {"disabled": true, "label": "Jump (cooling)"}, "coins": {"progress": 12}}}`. Game tokens only. Per control: `label` (1–40 characters, or `null` to restore the builder's label), `disabled` (viewers can't press it) and, for goals, `progress` (0 to the target). A message is applied in full or not at all. Publishing a new board version clears these.
- `{"type": "ready", "id": 3}`. Game tokens only. From each game connect, the board shows "Starting…" and refuses presses until the game says it is listening; send this after every `hello`. A game that disconnects (or misses two 30-second checks) stops holding the board.
- `{"type": "capture", "id": 5, "press": "…"}` / `{"type": "release", "id": 6, "press": "…"}`. Games and bridges. For a press with `confirm: true` (a "Game confirms" control), the viewer's Engagement Valor is only held: `capture` charges it once the effect happened, `release` refunds it. Unanswered presses are released after 60 seconds. Repeating the same answer is harmless; the opposite answer is refused.
- `{"type": "groups", "id": 7, "by": "faction", "screens": {"glint": "Red", "myria": "Blue"}}`. Game tokens only. Shows each group of viewers one screen of the published board (by its name) and refuses their presses on other screens: `by` is `"faction"` (`myria`/`aetheron`/`glint` → screen), `"random"` (2–4 screen names; each viewer lands on one, the same every time) or `"users"` (username → screen, up to 500). `"by": null` clears it; publishing a new version also clears it. Viewers in no group see every screen.
- `{"type": "cap", "id": 4, "per_second": 20}`. Game tokens only. The most presses and joystick moves a second sent to the game (1–100, or `null` for none); the streamer can also set it in Creator Studio. Over the cap, the newest input is refused and the viewer sees "Busy, try again".
- `{"type": "define", "id": 8, "name": "Crowd Runner", "board": {"screens": [{"name": "Game", "controls": [{"id": "jump", "kind": "button", "label": "Jump"}]}]}}`. Game tokens only, and only when the streamer switched on **May set my board** for the connection (Creator Studio → Board → Connections; for an app, Settings → Connected apps); otherwise it is refused with an `error` that says so. The board becomes the live board while the game is connected.
  - `board` has 1–4 `screens` (`name`, `controls`), up to 24 controls each. A control has `id` (lowercase letters, digits and `-`), `kind` (`button`, `label`, `text`, `goal`, `joystick`, `rally`), `label`, and optionally `cost`, `cooldown_seconds`, `per_stream_limit`, `audience` (`everyone`, `followers`, `subscribers`, `moderators`), `effect` (`none`, `confetti`, `hearts`, `fireworks`, `stars`, `rain`, `shake`, `spotlight`), `target` (goals), `width` (1–4) and `confirm`. It is checked like a board built in Creator Studio and applied in full or not at all.
  - It can't use the channel's own Skills as effects, and no `cost` may be more than the channel's typical viewer balance when S.V.E.R knows one (the `error` gives the number).
  - The first `define` of a connection saves the streamer's published board aside. It comes back as a new version when the game disconnects or misses two 30-second checks, or when the streamer presses "Take it back" (which also switches the permission off). So send `define` again after every `hello`.
  - At most one `define` every 10 seconds per connection. It is a normal publish: a new `version`, goals start over, game-set state and groups are cleared, and held presses on controls that are gone are released. The board is also saved to the streamer's library under `name`.

Close code `4001` means the token was revoked; don't reconnect. `4000` means the connection fell behind; reconnect.

Presses are already paid for (or, with `confirm: true`, held) and checked (real viewers only, cooldowns, limits, bans) before they reach you. Joystick moves are free and rate-limited to 10 a second per viewer.

## The OBS bridge

Requires Node 22 or newer and OBS 28 or newer (Tools → WebSocket Server Settings: enable it and set a password).

1. Copy `bridge/bridge.example.json` to `bridge/bridge.json` (ignored by git) and fill in the bridge token and OBS password.
2. Under `actions`, map each board control id (or `skill:<id>`, or `surge`) to steps: `{"scene": "..."}`, `{"source": "...", "scene": "...", "visible": true | false | "toggle"}`, `{"filter": "...", "source": "...", "enabled": true | false}`, `{"wait": ms}`. Add `"cooldown": ms` to limit how often a control can fire. For a "Game confirms" control the bridge captures the press when its steps ran and releases it (refunding the viewer) when they failed or the control was cooling down; controls it has no steps for are left to your game.
3. Run `node bridge/sver-bridge.mjs bridge/bridge.json`. It reconnects to S.V.E.R on its own.

## JavaScript and TypeScript

`sdk-js/sver-board.mjs` has no dependencies (types in `sver-board.d.ts`). Copy it into your game or import it by path:

```js
import { SverBoard } from "./sver-board.mjs";
const board = new SverBoard({ token });
board.on("press", press => { if (press.control === "jump") player.jump(); });
board.on("input", move => player.steer(move.x, move.y));
await board.connect();
await board.disable("jump");          // or label(), progress(), setState({...})
await board.define("My game", { screens: [{ name: "Game", controls: [{ id: "jump", kind: "button", label: "Jump" }] }] });
```

`define(name, board)` makes the game's own board live while it is connected. It rejects with the reason unless the streamer switched on "May set my board" for the token, so a game should fall back to the streamer's board.

To sign in instead of pasting a token, give the client ID:

```js
import { SverBoard, fileStore } from "./sver-board.mjs";
const board = new SverBoard({
  clientId: "YOUR_CLIENT_ID",
  onCode: ({ code, url, link }) => showMessage(`Enter ${code} at ${url}`),   // link opens the page with the code filled in
  // store: fileStore("sver-sign-in.json"),   // Node; browsers use localStorage by default
});
await board.connect();   // resolves once the streamer approved; later runs connect without a code
```

`connect()` calls `onCode` only when there is no working sign-in: the first time, after the streamer removed the game in Settings → Connected apps, or after 60 days without use. It rejects when the streamer chooses Cancel or S.V.E.R doesn't know the client ID. `signOut()` forgets the sign-in and ends it on S.V.E.R. `store` is any `{ get(), set(grant) }` (they may return promises); what it holds lets the game control that streamer's board, so keep it as private as a token.

The example game: serve the `integrations/` folder (for example `npx serve integrations`) and open `example-game/index.html#client=YOUR_CLIENT_ID`, then press **Connect to S.V.E.R**; or paste a game token into the field (`index.html#token=sver_g_…` also works). With "May set my board" on it defines its own board; otherwise the streamer's board needs a button `jump`, a goal `coins`, a text input `shout` and a joystick `move`.

## Unity

Add `sdk-unity` as a local package (Package Manager → Add package from disk → `package.json`). It depends on `com.unity.nuget.newtonsoft-json`. Add the **SverBoard** component, paste the token (or leave it empty, set **Client Id** and show the code from `SignInCode(code, url)`; the sign-in is kept in PlayerPrefs, `SignOut()` forgets it; this sign-in path has not been compiled or run yet), and subscribe to `Pressed`, `Moved`, `BoardChanged`, `StateChanged` and `Error`; call `SetDisabled`, `SetLabel`, `SetProgress` or `SetState`, and `Define(name, board)` to bring the game's own board. Events arrive on the main thread. For WebGL builds, use the JavaScript SDK from the hosting page.

## Unreal Engine

Copy `sdk-unreal/SverBoard` into your project's `Plugins/` folder and enable it. Create a `USverBoardClient` (Blueprint: Construct Object from Class), bind `OnPress`, `OnMove`, `OnBoardChanged` and `OnError`, then call `Connect` with the token, or `SignIn` with your client ID and show the code from `OnCode(Code, Url)` (the sign-in is kept in a file under Saved, or `SignInFile`; `SignOut` forgets it; this sign-in path has not been compiled or run yet). `SetDisabled`, `SetLabel`, `SetProgress` and `SetStateJson` update the board; `DefineJson(Name, BoardJson)` brings the game's own board.
