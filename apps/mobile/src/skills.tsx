import { useCallback, useEffect, useState } from "react";
import { Alert, Image, Modal, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "./api";
import { useAuth } from "./auth";
import { newId } from "./session";
import { Attempt, aboutValor } from "./spend";
import { fill, font, line, scrim, space } from "./theme";
import { Button, Field, Notice, Panel, Skeleton, bg, ink, inkMuted, type } from "./ui";
import { absolute } from "./urls";
import { GetValorSheet } from "./valor";

type Skill = { id: string; name: string; valor: number; enabled: boolean; image?: string | null };
type Skills = { skills: Skill[]; valor: number | null; paused: boolean };

/**
 * Skills (docs/CROWDSYNC.md "Skills"), as in the website's chat dock: pay Valor to play an effect
 * on stream and in chat. A Skill is sent as a chat message over HTTP, never the socket.
 */
export function SkillsSheet({ username, visible, onClose }: { username: string; visible: boolean; onClose: () => void }) {
  const { account, colors } = useAuth();
  const insets = useSafeAreaInsets();
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [data, setData] = useState<Skills | null>(null);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [refusal, setRefusal] = useState("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [getValor, setGetValor] = useState(false);
  const [attempt] = useState(() => new Attempt(newId));

  const load = useCallback(async () => {
    const result = await send<Skills>("GET", `${path}/skills`);
    if (!result.ok) { setError(result.error); return; }
    setError("");
    setData({ ...result.data, skills: Array.isArray(result.data.skills) ? result.data.skills.filter(s => s.enabled) : [] });
  }, [path]);
  useEffect(() => {
    // The list loads after the request; the call itself is not a render update.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (visible) void load();
  }, [visible, load, account?.id]);

  const close = () => { setNote(""); setRefusal(""); onClose(); };

  async function play(skill: Skill) {
    setBusy(true);
    setNote("");
    setRefusal("");
    const id = attempt.take(skill.id);
    const result = await send("POST", `${path}/chat`, { id, body: text.trim() || skill.name, skill: skill.id });
    // A retry whose first try went through: the server already has this message, charged once.
    const played = result.ok || (attempt.reused && result.status === 409);
    attempt.settle(result);
    setBusy(false);
    if (!played) { setRefusal(result.ok ? "" : result.error); return; }
    setText("");
    setNote(`${skill.name} played.`);
    setData(d => d && { ...d, valor: d.valor === null ? null : d.valor - skill.valor });
  }
  function confirm(skill: Skill) {
    Alert.alert(`Play ${skill.name} for ${skill.valor.toLocaleString()} Valor?`, undefined, [
      { text: "Cancel", style: "cancel" },
      { text: "Play", onPress: () => { void play(skill); } },
    ]);
  }

  const canPlay = !!account && data !== null && data.valor !== null;
  const border = line(colors.accent);
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={close} supportedOrientations={["portrait", "landscape"]}>
      <View style={styles.backdrop}>
        <Panel style={{ ...styles.sheet, paddingBottom: insets.bottom + space.m }}>
          <ScrollView contentContainerStyle={styles.gap} keyboardShouldPersistTaps="handled">
            <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">Skills</Text>
            {!account ? (
              <>
                <Text style={styles.body}>Log in to play Skills.</Text>
                <Button label="Log in" onPress={() => { close(); router.push("/sign-in"); }} />
              </>
            ) : error && !data ? (
              <Notice onRetry={() => { void load(); }}>{error}</Notice>
            ) : !data ? (
              <View style={styles.gap} accessible accessibilityLabel="Loading"><Skeleton w="70%" h={14} /><Skeleton w="100%" h={64} /></View>
            ) : (
              <>
                {data.valor === null ? (
                  <Text style={styles.body}>Streamers can&apos;t play Skills on their own channel.</Text>
                ) : (
                  <Text style={styles.body}>
                    <Text style={styles.strong}>{data.valor.toLocaleString()}</Text> Valor ·{" "}
                    <Text accessibilityRole="link" style={[styles.link, { color: colors.accentLight }]} onPress={() => setGetValor(true)}>Get Valor</Text>
                    . Skills show on stream and in chat and pay the streamer.
                  </Text>
                )}
                {data.paused ? <Notice>Effects are paused on this channel right now.</Notice> : null}
                {canPlay ? <Field label="Message (optional)" value={text} onChangeText={setText} maxLength={200} /> : null}
                <View style={styles.tiles}>
                  {data.skills.map(skill => {
                    const off = !canPlay || busy || data.paused || (data.valor ?? 0) < skill.valor;
                    return (
                      <Pressable key={skill.id} accessibilityRole="button" accessibilityLabel={`${skill.name}, ${skill.valor.toLocaleString()} Valor`} accessibilityState={{ disabled: off }}
                        disabled={off} onPress={() => confirm(skill)} style={({ pressed }) => [styles.tile, { borderColor: border }, pressed && styles.pressed, off && styles.off]}>
                        {skill.image ? <Image source={{ uri: absolute(skill.image) }} style={styles.art} accessibilityIgnoresInvertColors /> : null}
                        <Text style={styles.name} numberOfLines={2}>{skill.name}</Text>
                        <Text style={styles.cost}>{skill.valor.toLocaleString()} Valor</Text>
                      </Pressable>
                    );
                  })}
                </View>
              </>
            )}
            {note ? <Text style={styles.body} accessibilityRole="alert">{note}</Text> : null}
            {refusal ? <Notice>{refusal}</Notice> : null}
            {aboutValor(refusal) ? <Button label="Get Valor" tone="quiet" compact onPress={() => setGetValor(true)} /> : null}
            <Button label="Close" tone="quiet" onPress={close} />
          </ScrollView>
        </Panel>
        {/* Inside this Modal so it stacks over the sheet on iOS. */}
        <GetValorSheet visible={getValor} onClose={() => { setGetValor(false); void load(); }} />
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: scrim },
  sheet: { maxHeight: "85%", backgroundColor: bg, borderWidth: 0, borderTopWidth: 1, padding: space.m },
  gap: { gap: space.m },
  body: { ...type.body, color: inkMuted },
  strong: { fontFamily: font.bodySemi, color: ink },
  link: { textDecorationLine: "underline" },
  tiles: { flexDirection: "row", flexWrap: "wrap", gap: space.s },
  tile: { width: 100, minHeight: 88, borderWidth: 1, backgroundColor: fill, padding: space.s, gap: space.xs, alignItems: "center", justifyContent: "center" },
  art: { width: 32, height: 32 },
  name: { fontFamily: font.bodySemi, fontSize: 13, color: ink, textAlign: "center" },
  cost: { fontFamily: font.label, fontSize: 12, letterSpacing: 1, color: inkMuted },
  pressed: { opacity: 0.85 },
  off: { opacity: 0.5 },
});
