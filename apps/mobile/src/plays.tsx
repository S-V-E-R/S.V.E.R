import { useCallback, useEffect, useRef, useState } from "react";
import { AppState, Pressable, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { send } from "./api";
import { useAuth } from "./auth";
import { Bar } from "./crowd";
import { ArrowIcon } from "./icons";
import { fill, font, ink, inkDim, onAccent, space } from "./theme";
import { Button, Notice, Panel, Tag, type } from "./ui";

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
  const key = (command: Command) => {
    const chosen = state.my_vote === command;
    const color = chosen ? onAccent : colors.accentLight;
    return (
      <Pressable key={command} accessibilityRole="button" accessibilityLabel={`Vote ${command}`} accessibilityState={{ disabled, selected: chosen }}
        disabled={disabled} onPress={() => { void vote(command); }}
        style={({ pressed }) => [styles.key, { borderColor: colors.accent, backgroundColor: chosen || pressed ? colors.accent : fill, opacity: disabled && !chosen ? 0.55 : 1 }]}>
        {command === "up" || command === "down" || command === "left" || command === "right"
          ? <ArrowIcon dir={command} color={color} />
          : <Text style={[styles.keyText, { color }]}>{command.toUpperCase()}</Text>}
        <Text style={[styles.votes, { color: chosen ? onAccent : inkDim }]}>{state.votes.find(v => v.command === command)?.votes ?? 0}</Text>
      </Pressable>
    );
  };
  const status = state.my_vote ? `You voted ${state.my_vote.toUpperCase()}.` : state.can_vote ? "Your vote counts once, across buttons and chat." : state.why ?? (account ? "" : "Log in to vote.");
  return (
    <Panel>
      <View style={styles.rowBetween}>
        <Text style={type.label}>24/7 · SVER Plays</Text>
        <Tag label={!state.connected ? "Reconnecting" : state.input_mode === "rl" ? "Autonomous play" : "Viewer control"} />
      </View>
      <Text style={[type.display, styles.heading]} accessibilityRole="header">Play together</Text>
      <Text style={type.small}>{state.game}. Pick one button every five seconds; the most votes wins. You can also type a button&apos;s name in chat.</Text>
      {/* A d-pad on the left, A and B on the right, Start and Select under them: the Game Boy layout. */}
      <View style={styles.pad}>
        <View style={styles.dpad}>
          {key("up")}
          <View style={styles.keys}>{key("left")}<View style={styles.gap} />{key("right")}</View>
          {key("down")}
        </View>
        <View style={[styles.keys, styles.wide]}>{key("b")}{key("a")}</View>
      </View>
      <View style={[styles.keys, styles.wide, styles.menu]}>{key("select")}{key("start")}</View>
      <View style={styles.round}>
        <Text style={type.small}><Text style={styles.semi}>{(remaining / 1000).toFixed(1)}s</Text> until the next move</Text>
        <Bar share={remaining / 5000} color={colors.accent} label="Time left to vote" />
        {status ? <Text style={type.small}>{status}</Text> : null}
        {!account ? <Button label="Log in" tone="quiet" compact onPress={() => router.push("/sign-in")} /> : null}
        {state.last_command ? <Text style={type.small}>Last chosen: {state.last_command.toUpperCase()}</Text> : null}
      </View>
      <Text style={[type.small, styles.foot]}>Autonomous play keeps the game moving when no verified viewers are controlling it. Game votes don&apos;t award Valor or faction influence.</Text>
      {error ? <Notice>{error}</Notice> : null}
    </Panel>
  );
}

const styles = StyleSheet.create({
  heading: { marginVertical: space.xs },
  semi: { color: ink, fontFamily: font.bodySemi },
  rowBetween: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: space.s },
  pad: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", marginTop: space.l, paddingHorizontal: space.s },
  dpad: { alignItems: "center", gap: 6 },
  keys: { flexDirection: "row", gap: 6, alignItems: "center" },
  wide: { gap: 10 },
  menu: { justifyContent: "center", marginTop: 10 },
  gap: { width: 56 },
  key: { minWidth: 56, minHeight: 56, borderWidth: 1, alignItems: "center", justifyContent: "center", paddingHorizontal: space.s },
  keyText: { fontFamily: font.displayBold, fontSize: 16 },
  votes: { fontFamily: font.labelSemi, fontSize: 12 },
  round: { marginTop: space.m, gap: 6 },
  foot: { marginTop: space.s },
});
