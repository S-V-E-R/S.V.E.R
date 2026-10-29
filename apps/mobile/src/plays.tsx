import { useCallback, useEffect, useRef, useState } from "react";
import { AppState, Pressable, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { send } from "./api";
import { useAuth } from "./auth";
import { Frame, Notice, Tag, ink, inkDim, inkMuted } from "./ui";

type Command = "up" | "down" | "left" | "right" | "a" | "b" | "start" | "select";
type State = { game: string; round: number; closes_at: string; server_time: string; connected: boolean; input_mode: "chat" | "rl"; votes: { command: Command; votes: number }[]; my_vote: Command | null; can_vote: boolean; why: string | null; last_command: Command | null };

/**
 * The SVER Plays controller (apps/web/components/PlaysControls.tsx, docs/PLAYS.md): vote for one
 * button each five-second round; the most votes wins. Only Plays channels answer the endpoint, so
 * on any other channel this renders nothing and stops after the first request.
 */
export function PlaysControls({ username }: { username: string }) {
  const { account, colors } = useAuth();
  const path = `/api/channels/${encodeURIComponent(username)}/plays`;
  const [state, setState] = useState<State | null>(null);
  const [plays, setPlays] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [remaining, setRemaining] = useState(0);
  const load = useCallback(async () => {
    const r = await send<State>("GET", path);
    if (r.ok) { setState(r.data); setRemaining(Math.max(0, Date.parse(r.data.closes_at) - Date.parse(r.data.server_time))); }
    else if (r.status === 404) setPlays(false);
    else setError(r.error);
  }, [path]);
  const latest = useRef(load);
  useEffect(() => { latest.current = load; });
  useEffect(() => { void latest.current(); }, [path, account?.id]);
  useEffect(() => {
    if (!plays) return;
    const poll = setInterval(() => { if (AppState.currentState === "active") void latest.current(); }, 1000);
    const clock = setInterval(() => setRemaining(n => Math.max(0, n - 250)), 250);
    return () => { clearInterval(poll); clearInterval(clock); };
  }, [plays]);
  if (!plays || !state) return null;

  const disabled = busy || !state.connected || !state.can_vote || !!state.my_vote || remaining <= 0;
  async function vote(command: Command) {
    if (disabled || !state) return;
    setBusy(true);
    setError("");
    const r = await send("POST", path, { command, round: state.round });
    if (!r.ok) setError(r.error);
    await load();
    setBusy(false);
  }
  const key = (command: Command, label = command.toUpperCase()) => {
    const chosen = state.my_vote === command;
    return (
      <Pressable key={command} accessibilityRole="button" accessibilityLabel={`Vote ${command}`} accessibilityState={{ disabled, selected: chosen }}
        disabled={disabled} onPress={() => { void vote(command); }}
        style={({ pressed }) => [styles.key, { borderColor: colors.accent, backgroundColor: chosen || pressed ? colors.accent : "rgba(0,0,0,0.35)", opacity: disabled && !chosen ? 0.55 : 1 }]}>
        <Text style={[styles.keyText, { color: chosen ? "#0A0A0C" : colors.accentLight }]}>{label}</Text>
        <Text style={[styles.votes, { color: chosen ? "#0A0A0C" : inkDim }]}>{state.votes.find(v => v.command === command)?.votes ?? 0}</Text>
      </Pressable>
    );
  };
  const status = state.my_vote ? `You voted ${state.my_vote.toUpperCase()}.` : state.can_vote ? "Your vote counts once, across buttons and chat." : state.why ?? (account ? "" : "Log in to vote.");
  return (
    <Frame>
      <View style={styles.rowBetween}>
        <Text style={styles.eyebrow}>24/7 · SVER Plays</Text>
        <Tag label={!state.connected ? "Reconnecting" : state.input_mode === "rl" ? "Autonomous play" : "Viewer control"} />
      </View>
      <Text style={styles.heading}>Play together</Text>
      <Text style={styles.small}>{state.game}. Pick one button every five seconds; the most votes wins. You can also type a button&apos;s name in chat.</Text>
      {/* A d-pad on the left, A and B on the right, Start and Select under them: the Game Boy layout. */}
      <View style={styles.pad}>
        <View style={{ alignItems: "center", gap: 6 }}>
          {key("up", "↑")}
          <View style={{ flexDirection: "row", gap: 6 }}>{key("left", "←")}<View style={styles.gap} />{key("right", "→")}</View>
          {key("down", "↓")}
        </View>
        <View style={{ flexDirection: "row", gap: 10, alignItems: "center" }}>{key("b")}{key("a")}</View>
      </View>
      <View style={{ flexDirection: "row", gap: 10, justifyContent: "center", marginTop: 10 }}>{key("select", "SELECT")}{key("start", "START")}</View>
      <View style={{ marginTop: 12, gap: 6 }}>
        <Text style={styles.small}><Text style={{ color: ink, fontFamily: "Barlow_600SemiBold" }}>{(remaining / 1000).toFixed(1)}s</Text> until the next move</Text>
        <View accessibilityRole="progressbar" accessibilityLabel="Time left to vote" accessibilityValue={{ min: 0, max: 5000, now: remaining }} style={styles.bar}>
          <View style={{ width: `${Math.min(100, (remaining / 5000) * 100)}%`, height: "100%", backgroundColor: colors.accent }} />
        </View>
        {status ? <Text style={styles.small}>{status}</Text> : null}
        {!account ? <Pressable accessibilityRole="button" onPress={() => router.push("/sign-in")} style={styles.link}><Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Log in</Text></Pressable> : null}
        {state.last_command ? <Text style={styles.small}>Last chosen: {state.last_command.toUpperCase()}</Text> : null}
      </View>
      <Text style={[styles.small, { marginTop: 8 }]}>Autonomous play keeps the game moving when no verified viewers are controlling it. Game votes don&apos;t award Valor or faction influence.</Text>
      {error ? <Notice>{error}</Notice> : null}
    </Frame>
  );
}

const styles = StyleSheet.create({
  heading: { fontFamily: "Cinzel_800ExtraBold", color: ink, fontSize: 22, marginVertical: 4 },
  eyebrow: { fontFamily: "BarlowCondensed_700Bold", color: inkDim, letterSpacing: 1.6, textTransform: "uppercase", fontSize: 13 },
  small: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 14, lineHeight: 20 },
  rowBetween: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8 },
  pad: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", marginTop: 14, paddingHorizontal: 8 },
  gap: { width: 56 },
  key: { minWidth: 56, minHeight: 56, borderWidth: 1, alignItems: "center", justifyContent: "center", paddingHorizontal: 8 },
  keyText: { fontFamily: "Cinzel_700Bold", fontSize: 16 },
  votes: { fontFamily: "BarlowCondensed_600SemiBold", fontSize: 12 },
  bar: { height: 8, backgroundColor: "rgba(242,240,234,0.1)" },
  link: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
});
