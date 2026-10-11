// S.V.E.R for Stream Deck (Stream Deck 7.1+, its bundled Node 24, no dependencies). Stream Deck
// starts this with -port, -pluginUUID, -registerEvent and -info and talks to it over a local
// WebSocket. Sign-in tokens live in the plugin's global settings on this PC only.
import { Sver, SverError } from "./sver.mjs";

const arg = name => process.argv[process.argv.indexOf(`-${name}`) + 1];
const [port, uuid, registerEvent] = [arg("port"), arg("pluginUUID"), arg("registerEvent")];
const ws = new WebSocket(`ws://127.0.0.1:${port}`);
const send = message => ws.send(JSON.stringify(message));
const sver = new Sver({ save: tokens => send({ event: "setGlobalSettings", context: uuid, payload: { tokens } }) });
const PREFIX = "tv.sver.streamdeck.";
const reason = error => error instanceof SverError ? error.message : "Couldn't reach S.V.E.R.";

/** Runs one key press: a check mark when it worked, the warning triangle when not. */
async function press(action, context, settings, state) {
  try {
    switch (action.slice(PREFIX.length)) {
      case "title": await sver.setTitle(settings.title); break;
      case "raid": await sver.raid(settings.username); break;
      case "poll": await sver.poll(settings); break;
      case "marker": await sver.marker(settings.label); break;
      case "board": {
        // State 0 shows "running": pressing pauses the board; state 1 resumes it.
        const pause = state !== 1;
        await sver.boardPaused(pause);
        send({ event: "setState", context, payload: { state: pause ? 1 : 0 } });
        break;
      }
      default: return;
    }
    send({ event: "showOk", context });
  } catch (error) {
    send({ event: "showAlert", context });
    send({ event: "sendToPropertyInspector", action, context, payload: { error: reason(error) } });
  }
}

/** The property inspector's sign-in button: shows the code there and opens sver.tv/go. */
async function signIn(action, context) {
  const tell = payload => send({ event: "sendToPropertyInspector", action, context, payload });
  try {
    const { code, url, done } = await sver.signIn();
    tell({ code });
    send({ event: "openUrl", payload: { url } });
    await done;
    tell({ signedIn: await sver.me() });
  } catch (error) {
    tell({ error: reason(error) });
  }
}

ws.onopen = () => {
  send({ event: registerEvent, uuid });
  send({ event: "getGlobalSettings", context: uuid });
};
ws.onmessage = async ({ data }) => {
  const { event, action, context, payload = {} } = JSON.parse(data);
  if (event === "didReceiveGlobalSettings") sver.tokens = payload.settings?.tokens ?? null;
  else if (event === "keyDown") await press(action, context, payload.settings ?? {}, payload.state);
  else if (event === "sendToPlugin" && payload.signIn) await signIn(action, context);
  else if (event === "sendToPlugin" && payload.signOut) {
    sver.signOut();
    send({ event: "sendToPropertyInspector", action, context, payload: { signedIn: null } });
  } else if (event === "sendToPlugin" && payload.status) {
    const signedIn = sver.tokens ? await sver.me().catch(() => null) : null;
    send({ event: "sendToPropertyInspector", action, context, payload: { signedIn } });
  }
};
