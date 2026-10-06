# S.V.E.R integrations

Connect your stream's board ([docs/CROWDSYNC.md](../docs/CROWDSYNC.md)) to OBS and to games. Everything here talks only to S.V.E.R's integration gateway with a scoped token. Nothing connects to S.V.E.R's database or internal services.

| Folder | What it is | Status |
|---|---|---|
| [`bridge/`](bridge) | The S.V.E.R bridge: board presses switch OBS scenes, sources and filters | Tested (`node bridge/test.mjs`) |
| [`sdk-js/`](sdk-js) | JavaScript/TypeScript SDK for browser and Node games | Tested (`node sdk-js/test.mjs`) |
| [`example-game/`](example-game) | Crowd Runner, a small browser game built on the JS SDK | Runs in any browser |
| [`sdk-unity/`](sdk-unity) | Unity package (`tv.sver.board`) | Written against Unity 2021.3+ APIs; not yet compiled in CI |
| [`sdk-unreal/`](sdk-unreal) | Unreal Engine 5 plugin (`SverBoard`) | Written against UE 5.x APIs; not yet compiled in CI |

## Get a token

Creator Studio → Board → Connections → Create token. Choose **OBS bridge** or **Game**. The token is shown once; keep it out of source control. Disconnect it there any time: new connections stop at once and open ones close within 30 seconds.

## The gateway protocol

Connect a WebSocket to `wss://sver.tv/api/integrations/ws` with the token as `Authorization: Bearer <token>`, or as `?token=<token>` where headers aren't possible (browsers). Messages are JSON text frames.

From S.V.E.R:

| `type` | When | Fields |
|---|---|---|
| `hello` | On connect | `kind` (`bridge`/`game`), `channel`, `protocol` (1), `board` (the published board, or null), `version`, `disabled` (paused), `state`, `goals` |
| `board_effect` | A viewer pressed a control | `control`, `label`, `effect`, `user.username`, `text` (text inputs), `goal` (`progress`, `target`, `reached`), `stream_ms`, `at` |
| `board_effect` without `control` | A Skill, emote combo or Surge level played | `skill` or `surge`, `effect`, `caption` |
| `board_input` | A viewer moved a joystick | `control`, `x`, `y` (−1 to 1), `user.username` |
| `board` | A new version was published, or the board was paused or resumed | The same snapshot fields as `hello` |
| `board_state` | A game changed labels, availability or goal progress | `state`, `goals` |
| `ack` / `error` / `pong` | Replies to your messages | `id` (yours), `message` (errors) |

To S.V.E.R (at most 10 messages a second):

- `{"type": "ping", "id": 1}`
- `{"type": "state", "id": 2, "controls": {"jump": {"disabled": true, "label": "Jump (cooling)"}, "coins": {"progress": 12}}}`. Game tokens only. Per control: `label` (1–40 characters, or `null` to restore the builder's label), `disabled` (viewers can't press it) and, for goals, `progress` (0 to the target). A message is applied in full or not at all. Publishing a new board version clears these.

Close code `4001` means the token was revoked; don't reconnect. `4000` means the connection fell behind; reconnect.

Presses are already paid for and checked (real viewers only, cooldowns, limits, bans) before they reach you. Joystick moves are free and rate-limited to 10 a second per viewer.

## The OBS bridge

Requires Node 22 or newer and OBS 28 or newer (Tools → WebSocket Server Settings: enable it and set a password).

1. Copy `bridge/bridge.example.json` to `bridge/bridge.json` (ignored by git) and fill in the bridge token and OBS password.
2. Under `actions`, map each board control id (or `skill:<id>`, or `surge`) to steps: `{"scene": "..."}`, `{"source": "...", "scene": "...", "visible": true | false | "toggle"}`, `{"filter": "...", "source": "...", "enabled": true | false}`, `{"wait": ms}`. Add `"cooldown": ms` to limit how often a control can fire.
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
```

The example game: serve the `integrations/` folder (for example `npx serve integrations`) and open `example-game/index.html#token=sver_g_…`. Its board needs a button `jump`, a goal `coins`, a text input `shout` and a joystick `move`.

## Unity

Add `sdk-unity` as a local package (Package Manager → Add package from disk → `package.json`). It depends on `com.unity.nuget.newtonsoft-json`. Add the **SverBoard** component, paste the token, and subscribe to `Pressed`, `Moved`, `BoardChanged`, `StateChanged` and `Error`; call `SetDisabled`, `SetLabel`, `SetProgress` or `SetState`. Events arrive on the main thread. For WebGL builds, use the JavaScript SDK from the hosting page.

## Unreal Engine

Copy `sdk-unreal/SverBoard` into your project's `Plugins/` folder and enable it. Create a `USverBoardClient` (Blueprint: Construct Object from Class), bind `OnPress`, `OnMove`, `OnBoardChanged` and `OnError`, then call `Connect` with the token. `SetDisabled`, `SetLabel`, `SetProgress` and `SetStateJson` update the board.
