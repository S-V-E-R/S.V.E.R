import { useCallback, useEffect, useRef, useState } from "react";
import { AccessibilityInfo, DeviceEventEmitter, Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { router } from "expo-router";
import { send } from "./api";
import { useAuth } from "./auth";
import { blocker, counterText, type BoardView, type Control, type Counter } from "./board-rules";
import { newId } from "./session";
import { Frame, Notice, ink, inkDim, inkMuted } from "./ui";

/**
 * CrowdSync in the app (docs/CROWDSYNC.md): the channel's board, polls, predictions, counters,
 * Faction Rally and Surge, plus effects over the player. Live updates arrive on the chat socket
 * and are passed here as BOARD_EVENT, the website's "sver:board" event. Owner and moderator tools
 * (starting polls, resolving, counters, panic, board blocks) stay on the website.
 * Skills are never offered here: they spend Purchased Valor (docs/MOBILE.md, no purchases).
 */
export const BOARD_EVENT = "sver:board";
export const BOARD_TYPES = new Set(["board_effect", "board_input", "board", "poll", "counters", "rally", "surge", "board_state", "board_result"]);
type BoardEvent = {
  type: string; control?: string; label?: string; effect?: string; user?: { username: string }; text?: string | null;
  goal?: { progress: number; target: number; reached: boolean } | null; overlay?: boolean; test?: boolean; caption?: string;
  id?: string; outcome?: "captured" | "released"; state?: BoardView["state"]; goals?: Record<string, number>;
  poll?: PollState; counters?: Counter[]; rally?: Rally | null; surge?: Surge;
};
type Detail = { channel: string; event: BoardEvent };

/** Listens to this channel's live CrowdSync events. */
function useBoardEvents(username: string, handle: (event: BoardEvent) => void) {
  const latest = useRef(handle);
  useEffect(() => { latest.current = handle; });
  useEffect(() => {
    const sub = DeviceEventEmitter.addListener(BOARD_EVENT, (detail: Detail) => {
      if (detail.channel === username.toLowerCase()) latest.current(detail.event);
    });
    return () => sub.remove();
  }, [username]);
}

// How far behind live this phone's player is, in ms, so effects and poll windows follow stream time.
// The player measures it from the HLS program date (currentOffsetFromLive); 4 s until it knows.
let delayMs = 4000;
export function setPlayerDelay(ms: number) { delayMs = ms; }

// ---- Board ----

const AUDIENCE = { everyone: "", followers: "Followers", subscribers: "Subscribers", moderators: "Moderators" };
const STICK: [string, string, number, number][] = [["up", "↑", 0, -1], ["left", "←", -1, 0], ["right", "→", 1, 0], ["down", "↓", 0, 1]];


export function Board({ username }: { username: string }) {
  const { colors } = useAuth();
  const path = `/api/channels/${encodeURIComponent(username)}/board`;
  const [view, setView] = useState<BoardView | null>(null);
  const [screen, setScreen] = useState(0);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const lastMove = useRef(0);
  // "Game confirms" presses waiting on the game: press ID → control label.
  const held = useRef(new Map<string, string>());
  const load = useCallback(async () => {
    const r = await send<BoardView>("GET", path);
    if (r.ok) setView(r.data); else setNote(r.error);
  }, [path]);
  const first = useRef(load);
  useEffect(() => { first.current = load; });
  useEffect(() => {
    void first.current();
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, [path]);
  useBoardEvents(username, e => {
    if (e.type === "board") void load();
    else if (e.type === "board_state") setView(v => v && { ...v, state: e.state, goals: e.goals ?? v.goals });
    else if (e.type === "board_result" && e.id) {
      const label = held.current.get(e.id);
      if (label === undefined) return;
      held.current.delete(e.id);
      setNote(e.outcome === "captured" ? `${label}: the game took it.` : `${label}: the game didn't take it, so your Engagement Valor was refunded.`);
      if (e.outcome === "released") void load();
    } else if (e.type === "board_effect" && e.goal && e.control) {
      const id = e.control, progress = e.goal.progress;
      setView(v => v && { ...v, goals: { ...v.goals, [id]: progress } });
    }
  });

  async function press(control: Control, extra: Record<string, unknown> = {}) {
    if (!view) return;
    setBusy(control.kind !== "joystick");
    setNote("");
    const id = newId();
    if (control.confirm) held.current.set(id, control.label);
    const r = await send<{ balance?: number; held?: boolean }>("POST", `${path}/press`, { id, version: view.version, control: control.id, ...extra });
    setBusy(false);
    if (!r.ok) {
      held.current.delete(id);
      setNote(r.error);
      if (r.status === 409) void load();
      return;
    }
    if (control.kind === "joystick") return;
    if (r.data.held) setNote(`${control.label}: waiting for the game…`);
    setView(v => v && { ...v, balance: r.data.balance ?? v.balance, last_press: { ...v.last_press, [control.id]: new Date().toISOString() }, used: { ...v.used, [control.id]: (v.used[control.id] ?? 0) + 1 } });
    if (control.kind === "text") setTexts(t => ({ ...t, [control.id]: "" }));
  }
  function move(control: Control, x: number, y: number, at: number) {
    // The server allows 10 moves a second; stay under it (`at` is the tap's timestamp).
    if (at - lastMove.current < 120) return;
    lastMove.current = at;
    void press(control, { x, y });
  }

  const board = view?.board;
  if (!view || !board) return null;
  // A game may put the viewer in a group that sees one screen.
  const grouped = view.screen ?? null;
  const current = board.screens[Math.min(grouped ?? screen, board.screens.length - 1)];
  const canPress = view.signed_in && view.live && !view.disabled && !view.starting && view.balance !== null;
  const price = (c: Control) => (c.cost ? `${c.cost.toLocaleString()} EV` : "Free") + (AUDIENCE[c.audience] ? ` · ${AUDIENCE[c.audience]}` : "");
  const status = view.disabled ? "The board is paused." : view.starting ? "Starting… the game isn't listening yet." : !view.live ? "The board works while the stream is live."
    : !view.signed_in ? "Log in and watch to use the board." : view.balance === null ? "This is your board. Test it in Creator Studio on the website."
    : "Presses spend this channel's Engagement Valor, earned by watching and chatting.";
  const line = `${colors.accent}52`;

  return (
    <Frame>
      <View style={styles.rowBetween}>
        <Text style={styles.heading}>Board</Text>
        {view.balance != null ? <Text style={styles.small}>{view.balance.toLocaleString()} Engagement Valor</Text> : null}
      </View>
      <Text style={styles.small}>{status}</Text>
      {!view.signed_in ? <Pressable accessibilityRole="button" onPress={() => router.push("/sign-in")} style={styles.link}><Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Log in</Text></Pressable> : null}
      {grouped !== null && current ? <Text style={styles.small}>Your group: {current.name}</Text> : null}
      {grouped === null && board.screens.length > 1 ? (
        <View style={styles.tabs} accessibilityRole="tablist">
          {board.screens.map((s, i) => (
            <Pressable key={s.name + i} accessibilityRole="tab" accessibilityState={{ selected: i === screen }} onPress={() => setScreen(i)}
              style={[styles.tab, { borderColor: colors.accent, backgroundColor: i === screen ? colors.accent : "transparent" }]}>
              <Text style={[styles.tabText, { color: i === screen ? "#0A0A0C" : colors.accentLight }]}>{s.name}</Text>
            </Pressable>
          ))}
        </View>
      ) : null}
      <View style={styles.grid}>
        {current?.controls.map(base => ({ ...base, label: view.state?.[base.id]?.label ?? base.label })).map(c => {
          const why = blocker(c, view, now);
          const off = !canPress || busy || !!why;
          // Phones get two columns, like the website below 600 px: wide controls take the whole row.
          const span = { width: c.width >= 2 || c.kind !== "button" ? "100%" : "48.5%" } as const;
          if (c.kind === "label") return <Text key={c.id} style={[styles.label, span]}>{c.label}</Text>;
          if (c.kind === "joystick") return (
            <View key={c.id} style={[span, { alignItems: "center", gap: 4 }]} accessibilityLabel={c.label}>
              <Text style={styles.small}>{c.label}</Text>
              {[[STICK[0]], [STICK[1], STICK[2]], [STICK[3]]].map((row, r) => (
                <View key={r} style={{ flexDirection: "row", gap: 48 }}>
                  {row.map(([name, arrow, x, y]) => (
                    <Pressable key={name} accessibilityRole="button" accessibilityLabel={`${c.label} ${name}`} disabled={!canPress} onPress={e => move(c, x, y, e.nativeEvent.timestamp)}
                      style={[styles.stick, { borderColor: colors.accent, opacity: canPress ? 1 : 0.5 }]}>
                      <Text style={[styles.stickText, { color: colors.accentLight }]}>{arrow}</Text>
                    </Pressable>
                  ))}
                </View>
              ))}
            </View>
          );
          if (c.kind === "text") return (
            <View key={c.id} style={[span, { gap: 6 }]}>
              <Text style={styles.small}>{c.label} · {price(c)}</Text>
              <TextInput accessibilityLabel={c.label} value={texts[c.id] ?? ""} maxLength={200} editable={canPress} onChangeText={t => setTexts(all => ({ ...all, [c.id]: t }))}
                style={[styles.input, { borderColor: line }]} />
              <ControlButton label={why ?? "Send"} detail={null} disabled={off || !(texts[c.id] ?? "").trim()} onPress={() => { void press(c, { text: texts[c.id] ?? "" }); }} accent={colors} />
            </View>
          );
          if (c.kind === "goal") {
            const progress = view.goals[c.id] ?? 0;
            return (
              <View key={c.id} style={[span, { gap: 6 }]}>
                <Text style={styles.small}>{c.label} · {progress.toLocaleString()} / {c.target?.toLocaleString()}</Text>
                <Bar share={c.target ? progress / c.target : 0} color={colors.accent} label={c.label} />
                <ControlButton label={why ?? "Add"} detail={why ? null : price(c)} disabled={off} onPress={() => { void press(c); }} accent={colors} />
              </View>
            );
          }
          return <View key={c.id} style={span}><ControlButton label={c.label} detail={why ?? price(c)} disabled={off} onPress={() => { void press(c); }} accent={colors} /></View>;
        })}
      </View>
      {note ? <Text style={styles.small} accessibilityRole="alert">{note}</Text> : null}
    </Frame>
  );
}

type Accent = { accent: string; accentLight: string; accentDark: string };
function ControlButton({ label, detail, disabled, onPress, accent }: { label: string; detail: string | null; disabled: boolean; onPress: () => void; accent: Accent }) {
  return (
    <Pressable accessibilityRole="button" accessibilityState={{ disabled }} disabled={disabled} onPress={onPress}
      style={({ pressed }) => [styles.control, { borderColor: accent.accentLight, borderBottomColor: accent.accentDark, backgroundColor: pressed ? accent.accentDark : accent.accent, opacity: disabled ? 0.5 : 1 }]}>
      <Text style={styles.controlText}>{label}</Text>
      {detail ? <Text style={styles.controlDetail}>{detail}</Text> : null}
    </Pressable>
  );
}

function Bar({ share, color, label }: { share: number; color: string; label: string }) {
  const pct = Math.max(0, Math.min(100, Math.round(share * 100)));
  return (
    <View accessibilityRole="progressbar" accessibilityLabel={label} accessibilityValue={{ min: 0, max: 100, now: pct }} style={styles.bar}>
      <View style={{ width: `${pct}%`, height: "100%", backgroundColor: color }} />
    </View>
  );
}

// ---- Polls, predictions, counters, rally and Surge ----

type PollState = {
  id: string; kind: "poll" | "prediction"; question: string; options: string[]; ends_at: string; grace_seconds: number;
  status: "open" | "ended" | "locked" | "resolved" | "cancelled"; winner: number | null; counts: number[]; pools: number[] | null;
  mine?: { option: number; stake: number; payout: number | null } | null;
};
type Rally = { myria: number; aetheron: number; glint: number };
type Surge = { level: number; threshold: number; ends_at: string; ended_at: string | null; participants: number };
type State = { poll: PollState | null; prediction: PollState | null; counters: Counter[]; balance: number | null; max_stake: number; rally: Rally | null; surge: Surge | null; faction: keyof Rally | null; own?: boolean };
const FACTIONS: [keyof Rally, string, string][] = [["myria", "Myria", "#FF9A1F"], ["aetheron", "Aetheron", "#A68BFF"], ["glint", "Glint", "#E9C35A"]];


export function Crowd({ username }: { username: string }) {
  const { account, colors } = useAuth();
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [state, setState] = useState<State | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [rallyNote, setRallyNote] = useState("");
  const load = useCallback(async () => {
    const r = await send<State>("GET", `${path}/crowd`);
    if (r.ok) setState(r.data);
  }, [path]);
  const first = useRef(load);
  useEffect(() => { first.current = load; });
  useEffect(() => {
    void first.current();
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, [path, account?.id]);
  useBoardEvents(username, e => {
    if (e.type === "counters" && e.counters) { const counters = e.counters; setState(s => s && { ...s, counters }); }
    else if (e.type === "rally") { const rally = e.rally ?? null; setState(s => s && { ...s, rally }); }
    else if (e.type === "surge" && e.surge) { const surge = e.surge.ended_at ? null : e.surge; setState(s => s && { ...s, surge }); }
    else if (e.type === "poll" && e.poll) {
      const poll = e.poll;
      // Live results; the viewer's own vote, stake and payout come from a reload when they change.
      setState(s => s && { ...s, [poll.kind]: { ...poll, mine: s[poll.kind]?.id === poll.id ? s[poll.kind]?.mine : null } });
      if (poll.status === "resolved" || poll.status === "cancelled") void load();
    }
  });
  if (!state) return null;
  async function rally() {
    const r = await send("POST", `${path}/rally`);
    setRallyNote(r.ok ? "Rallied! You can rally again in a minute." : r.error);
    if (r.ok) void load();
  }
  const surgeLeft = state.surge ? Math.max(0, Math.ceil((Date.parse(state.surge.ends_at) - now) / 1000)) : 0;
  const total = state.rally ? state.rally.myria + state.rally.aetheron + state.rally.glint : 0;
  return (
    <View style={{ gap: 16 }}>
      {state.surge ? (
        <Frame>
          <Text style={styles.heading} accessibilityRole="alert">Surge level {state.surge.level}</Text>
          <Text style={styles.small}>{state.surge.participants} taking part · next level at {state.surge.threshold * (state.surge.level + 1)} · {Math.floor(surgeLeft / 60)}:{String(surgeLeft % 60).padStart(2, "0")} left</Text>
        </Frame>
      ) : null}
      {state.rally ? (
        <Frame>
          <Text style={styles.eyebrow}>Faction rally</Text>
          {FACTIONS.map(([key, name, color]) => {
            const share = total ? Math.round((state.rally![key] / total) * 100) : 0;
            return (
              <View key={key} style={styles.rallyRow}>
                <Text style={[styles.small, { width: 72 }]}>{name}</Text>
                <View style={{ flex: 1 }}><Bar share={share / 100} color={color} label={`${name}: ${share}%`} /></View>
                <Text style={[styles.small, { width: 40, textAlign: "right" }]}>{share}%</Text>
              </View>
            );
          })}
          {state.own ? <Text style={styles.small}>Your viewers rally for their factions here.</Text>
            : state.faction ? <ControlButton label={`Rally for ${FACTIONS.find(f => f[0] === state.faction)?.[1]}`} detail={null} disabled={false} onPress={() => { void rally(); }} accent={colors} />
            : account ? <Text style={styles.small}>Pick a side on the website to rally.</Text>
            : <Text style={styles.small}>Log in to rally for your faction.</Text>}
          {rallyNote ? <Text style={styles.small} accessibilityRole="alert">{rallyNote}</Text> : null}
        </Frame>
      ) : null}
      {state.counters.length > 0 ? (
        <Frame>
          {state.counters.map(c => <Text key={c.id} style={styles.body}>{c.label}: <Text style={{ fontFamily: "Barlow_600SemiBold" }}>{counterText(c)}</Text></Text>)}
        </Frame>
      ) : null}
      {state.poll ? <PollCard poll={state.poll} path={path} state={state} now={now} onChange={load} /> : null}
      {state.prediction ? <PollCard poll={state.prediction} path={path} state={state} now={now} onChange={load} /> : null}
    </View>
  );
}

function PollCard({ poll, path, state, now, onChange }: { poll: PollState; path: string; state: State; now: number; onChange: () => void }) {
  const { colors } = useAuth();
  const [stake, setStake] = useState(() => String(Math.max(1, Math.min(100, state.max_stake, state.balance ?? 100))));
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  // The window closes by stream time: when this viewer's own video reaches the end.
  const left = Math.ceil((Date.parse(poll.ends_at) + Math.min(delayMs, (poll.grace_seconds - 1) * 1000) - now) / 1000);
  const open = poll.status === "open" && left > 0;
  const prediction = poll.kind === "prediction";
  async function vote(option: number) {
    setBusy(true);
    setNote("");
    const r = await send("POST", `${path}/polls/${poll.id}/vote`, { option, stake: prediction ? Number(stake) : undefined });
    setBusy(false);
    if (!r.ok) setNote(r.error);
    onChange();
  }
  const status = poll.status === "resolved" ? `Result: ${poll.options[poll.winner ?? 0]}` : poll.status === "cancelled" ? "Cancelled; stakes were refunded."
    : poll.status === "locked" ? "Locked. Waiting for the result." : open ? `${left}s left` : "Voting has closed.";
  const total = poll.counts.reduce((a, b) => a + b, 0);
  return (
    <Frame>
      <Text style={styles.eyebrow}>{prediction ? "Prediction · Engagement Valor" : "Poll"}</Text>
      <Text style={styles.question}>{poll.question}</Text>
      <Text style={styles.small} accessibilityRole="text">{status}{poll.mine?.payout != null ? ` · You got back ${poll.mine.payout.toLocaleString()} EV` : ""}</Text>
      {open && !poll.mine && state.balance !== null ? (
        <View style={{ gap: 8, marginTop: 8 }}>
          {prediction ? (
            <View style={{ gap: 4 }}>
              <Text style={styles.small} nativeID={`stake-${poll.id}`}>Stake (you have {state.balance.toLocaleString()})</Text>
              <TextInput accessibilityLabelledBy={`stake-${poll.id}`} keyboardType="number-pad" value={stake} onChangeText={setStake} style={[styles.input, { borderColor: `${colors.accent}52` }]} />
            </View>
          ) : null}
          {poll.options.map((option, i) => <ControlButton key={option} label={option} detail={null} disabled={busy} onPress={() => { void vote(i); }} accent={colors} />)}
        </View>
      ) : (
        <View style={{ gap: 8, marginTop: 8 }}>
          {poll.options.map((option, i) => {
            const share = total ? Math.round((poll.counts[i] / total) * 100) : 0;
            return (
              <View key={option} style={{ gap: 4 }}>
                <View style={styles.rowBetween}>
                  <Text style={styles.body}>{poll.winner === i ? "✓ " : ""}{option}{poll.mine?.option === i ? " (you)" : ""}</Text>
                  <Text style={styles.small}>{poll.pools ? `${poll.pools[i].toLocaleString()} EV · ` : ""}{poll.counts[i]} ({share}%)</Text>
                </View>
                <Bar share={share / 100} color={colors.accent} label={`${option}: ${share}%`} />
              </View>
            );
          })}
        </View>
      )}
      {note ? <Notice>{note}</Notice> : null}
    </Frame>
  );
}

// ---- Effects over the player ----

// The website's effect marks (apps/web/components/BoardEffects.tsx).
const PARTICLES: Record<string, string> = { confetti: "🎉", hearts: "❤️", fireworks: "🎆", stars: "⭐", rain: "💧" };
const STICKERS: Record<string, string> = { "sticker-crown": "👑", "sticker-heart": "💖", "sticker-trophy": "🏆" };
type Shown = BoardEvent & { key: number };

/**
 * Board presses, emote combos, Surge levels and Skills over the video, shown when this phone's
 * picture reaches the moment they happened. Skipped when the streamer's OBS overlay already shows
 * them inside the video, and never for test presses. Reduce Motion keeps the captions only.
 * ponytail: sounds (Skill chimes, custom Skill audio) don't play in the app yet; the caption still shows.
 */
export function PlayerEffects({ username }: { username: string }) {
  const [shown, setShown] = useState<Shown[]>([]);
  const [calm, setCalm] = useState(false);
  const next = useRef(0);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  useEffect(() => {
    void AccessibilityInfo.isReduceMotionEnabled().then(setCalm);
    const sub = AccessibilityInfo.addEventListener("reduceMotionChanged", setCalm);
    const pending = timers.current;
    return () => { sub.remove(); pending.forEach(clearTimeout); };
  }, []);
  useBoardEvents(username, e => {
    if (e.type !== "board_effect" || e.overlay || e.test) return;
    timers.current.push(setTimeout(() => {
      const key = ++next.current;
      setShown(list => [...list.slice(-2), { ...e, key }]);
      timers.current.push(setTimeout(() => setShown(list => list.filter(x => x.key !== key)), 4000));
    }, delayMs));
  });
  if (!shown.length) return null;
  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="none" accessibilityLiveRegion="polite">
      {shown.map(e => {
        const animate = !calm && !!e.effect && e.effect !== "none" && (!e.goal || e.goal.reached);
        const mark = animate ? PARTICLES[e.effect!] ?? STICKERS[e.effect!] : undefined;
        const caption = e.caption ?? `${e.user?.username ?? "Someone"} ${e.goal ? (e.goal.reached ? `completed ${e.label}!` : `added to ${e.label} (${e.goal.progress}/${e.goal.target})`) : `used ${e.label}`}${e.text ? `: "${e.text}"` : ""}`;
        return (
          <View key={e.key} style={styles.effect}>
            {mark ? <Text style={styles.effectMark} accessibilityElementsHidden importantForAccessibility="no">{STICKERS[e.effect!] ? mark : mark.repeat(5)}</Text> : null}
            <Text style={styles.effectCaption}>{caption}</Text>
          </View>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  heading: { fontFamily: "Cinzel_800ExtraBold", color: ink, fontSize: 22 },
  eyebrow: { fontFamily: "BarlowCondensed_700Bold", color: inkDim, letterSpacing: 1.6, textTransform: "uppercase", fontSize: 13, marginBottom: 6 },
  question: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 17, lineHeight: 23 },
  body: { color: ink, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  small: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 14, lineHeight: 20 },
  rowBetween: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8 },
  link: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
  tabs: { flexDirection: "row", flexWrap: "wrap", gap: 8, marginTop: 10 },
  tab: { minHeight: 44, borderWidth: 1, paddingHorizontal: 14, justifyContent: "center" },
  tabText: { fontFamily: "Cinzel_700Bold", fontSize: 14 },
  grid: { flexDirection: "row", flexWrap: "wrap", justifyContent: "space-between", rowGap: 8, marginTop: 12 },
  label: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 15, paddingVertical: 6 },
  control: { minHeight: 52, borderWidth: 1, borderBottomWidth: 2, alignItems: "center", justifyContent: "center", paddingHorizontal: 8, paddingVertical: 6 },
  controlText: { color: "#0A0A0C", fontFamily: "Cinzel_700Bold", fontSize: 15, letterSpacing: 1, textAlign: "center" },
  controlDetail: { color: "#0A0A0C", fontFamily: "BarlowCondensed_600SemiBold", fontSize: 12, letterSpacing: 0.6 },
  stick: { width: 52, height: 52, borderWidth: 1, alignItems: "center", justifyContent: "center", backgroundColor: "rgba(0,0,0,0.35)" },
  stickText: { fontSize: 22, fontFamily: "Barlow_600SemiBold" },
  input: { borderWidth: 1, backgroundColor: "rgba(0,0,0,0.35)", minHeight: 44, paddingHorizontal: 12, color: ink, fontFamily: "Barlow_400Regular", fontSize: 16 },
  bar: { height: 8, backgroundColor: "rgba(242,240,234,0.1)" },
  rallyRow: { flexDirection: "row", alignItems: "center", gap: 8, marginVertical: 3 },
  effect: { position: "absolute", left: 8, right: 8, bottom: 8, alignItems: "center", gap: 4 },
  effectMark: { fontSize: 28 },
  effectCaption: { color: ink, backgroundColor: "rgba(0,0,0,0.7)", fontFamily: "Barlow_600SemiBold", fontSize: 14, paddingHorizontal: 10, paddingVertical: 4, textAlign: "center" },
});
