/** OBS helpers shared by Creator Studio and the OBS dock (`/dock`). */

export type Broadcast = { state: string; observed_at: string | null };

/** What Studio and the dock say about the broadcast. A LIVE broadcast with no media for 15 seconds is "Signal not confirmed". */
export function onAir(broadcast: Broadcast | null, disconnectPending: boolean, hasKey: boolean, seenAt: number) {
  const stale = broadcast?.state === "LIVE" && (!broadcast.observed_at || seenAt - Date.parse(broadcast.observed_at) > 15000);
  const state = disconnectPending ? "Disconnecting" : stale ? "Signal not confirmed" : ({ STARTING: "Connecting", LIVE: "Live", RECONNECTING: "Reconnecting", ENDED: "Offline" }[broadcast?.state ?? "ENDED"] ?? "Offline");
  const plain = state === "Offline" ? (hasKey ? "OBS isn't connected yet. Press Start Streaming in OBS" : "OBS isn't connected yet") : state === "Live" ? "You're live" : state === "Connecting" ? "OBS is connecting…" : state === "Reconnecting" ? "Connection dropped. Waiting for OBS to reconnect" : state === "Signal not confirmed" ? "Connected, but no fresh video yet" : state;
  return { state, plain, stale, live: broadcast?.state === "LIVE" && !stale && !disconnectPending };
}

export type Os = "windows" | "mac" | "linux";
export const osOf = (userAgent: string): Os => /Windows/.test(userAgent) ? "windows" : /Mac/.test(userAgent) ? "mac" : "linux";

// OBS's own source ids per platform (obs-studio's default audio devices).
const MIC: Record<Os, string> = { windows: "wasapi_input_capture", mac: "coreaudio_input_capture", linux: "pulse_input_capture" };
const DESKTOP: Record<Os, string | null> = { windows: "wasapi_output_capture", mac: null, linux: "pulse_output_capture" };

type Item = { source: string; x: number; y: number; hidden?: boolean; fit?: boolean };

/**
 * An OBS scene collection (Scene Collection → Import) for a 1920×1080 canvas: a "Live" scene with the
 * "S.V.E.R overlays" scene nested on top, which holds the alert box, goal bar, recent events (hidden) and
 * the chat overlay. On Windows "Live" starts with Game Capture of any fullscreen game; a display or window
 * capture can't be preset, because OBS needs the device id, so elsewhere the streamer adds their own.
 */
export function sceneCollection({ alerts, chat, os }: { alerts: string; chat: string; os: Os }) {
  const uuid = new Map<string, string>();
  const sources: Record<string, unknown>[] = [];
  const add = (name: string, id: string, settings: Record<string, unknown>, extra: Record<string, unknown> = {}) => {
    uuid.set(name, crypto.randomUUID());
    sources.push({ name, uuid: uuid.get(name), id, versioned_id: id, settings, enabled: true, volume: 1, mixers: 255, ...extra });
  };
  // Sound goes through OBS so it reaches the stream, and plays on the streamer's monitor too.
  const browser = (name: string, url: string, width: number, height: number, sound = false) =>
    add(name, "browser_source", { url, width, height, reroute_audio: sound, shutdown: false, restart_when_active: false }, sound ? { monitoring_type: 2 } : {});
  const scene = (name: string, items: Item[]) => add(name, "scene", {
    id_counter: items.length,
    custom_size: false,
    items: items.map((item, i) => ({
      name: item.source, source_uuid: uuid.get(item.source), id: i + 1, visible: !item.hidden, locked: false,
      pos: { x: item.x, y: item.y }, scale: { x: 1, y: 1 }, rot: 0, align: 5,
      // Fit the capture to the canvas whatever the game's resolution.
      bounds_type: item.fit ? 2 : 0, bounds_align: 0, bounds: item.fit ? { x: 1920, y: 1080 } : { x: 0, y: 0 },
    })),
  }, { mixers: 0 });

  browser("S.V.E.R Alerts", alerts, 1920, 1080, true);
  browser("S.V.E.R Goal bar", `${alerts}?widget=goal`, 800, 110);
  browser("S.V.E.R Recent events", `${alerts}?widget=events`, 420, 400);
  browser("S.V.E.R Chat", chat, 420, 640);
  scene("S.V.E.R overlays", [
    { source: "S.V.E.R Alerts", x: 0, y: 0 },
    { source: "S.V.E.R Goal bar", x: 560, y: 950 },
    { source: "S.V.E.R Recent events", x: 20, y: 20, hidden: true },
    { source: "S.V.E.R Chat", x: 1480, y: 420 },
  ]);
  if (os === "windows") add("Game", "game_capture", { capture_mode: "any_fullscreen" });
  scene("Live", [...(os === "windows" ? [{ source: "Game", x: 0, y: 0, fit: true }] : []), { source: "S.V.E.R overlays", x: 0, y: 0 }]);

  const device = (name: string, id: string) => ({ name, uuid: crypto.randomUUID(), id, versioned_id: id, settings: { device_id: "default" }, enabled: true, volume: 1, mixers: 255 });
  const desktop = DESKTOP[os];
  return {
    name: "S.V.E.R",
    sources,
    groups: [],
    scene_order: [{ name: "Live" }, { name: "S.V.E.R overlays" }],
    current_scene: "Live",
    current_program_scene: "Live",
    current_transition: "Fade",
    transition_duration: 300,
    transitions: [],
    quick_transitions: [],
    modules: {},
    ...(desktop ? { DesktopAudioDevice1: device("Desktop Audio", desktop) } : {}),
    AuxAudioDevice1: device("Mic/Aux", MIC[os]),
  };
}
