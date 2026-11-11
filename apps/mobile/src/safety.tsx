import { useState } from "react";
import { Alert, Modal, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { send } from "./api";
import { useAuth } from "./auth";
import { Button, Frame, Notice, bg, ink, inkMuted } from "./ui";

/** The website's report reasons (apps/web/lib/types.ts), checked again by the API (safety.rs REASONS). */
export const REASONS: [string, string][] = [
  ["spam", "Spam"], ["harassment", "Harassment or bullying"], ["hate", "Hate speech"], ["sexual", "Sexual content"],
  ["violence", "Violence or threats"], ["impersonation", "Impersonation"], ["private_information", "Private information"],
  ["copyright", "Copyright"], ["mature_label", "Should be labeled mature"], ["other", "Something else"],
];

/** What is being reported: a chat message, a Beacon or a live stream, and who posted it. */
export type SafetyTarget = { type: "chat_message" | "beacon" | "live_stream"; id: string; label: string; username: string | null; displayName: string };

/**
 * Report and block, required by both app stores for chat and videos. Reports go to the same staff
 * queue as the website's; blocking hides the person everywhere on S.V.E.R.
 */
export function SafetySheet({ target, onClose, onBlocked }: { target: SafetyTarget | null; onClose: () => void; onBlocked?: (username: string) => void }) {
  const { account, colors } = useAuth();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState("");
  const [error, setError] = useState("");
  const close = () => { setDone(""); setError(""); onClose(); };

  async function report(reason: string) {
    if (!target) return;
    setBusy(true);
    setError("");
    const result = await send<{ message?: string }>("POST", "/api/reports", { target_type: target.type, target_id: target.id, reason, note: "" });
    setBusy(false);
    if (result.ok) setDone(result.data.message ?? "Thanks. Staff will review your report.");
    else setError(result.error);
  }

  function block() {
    const name = target?.username;
    if (!name) return;
    Alert.alert(`Block ${target.displayName}?`, "You won't see each other's chat messages, Beacons or streams, and they can't follow or message you.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Block", style: "destructive", onPress: () => {
          void send("PUT", `/api/blocks/${encodeURIComponent(name)}`).then(result => {
            if (!result.ok) { setError(result.error); return; }
            onBlocked?.(name);
            setDone(`You blocked ${target.displayName}. Unblock from Settings on the website.`);
          });
        },
      },
    ]);
  }

  // --line in docs/DESIGN.md: the accent at 32% opacity.
  const line = `${colors.accent}52`;
  return (
    <Modal visible={!!target} transparent animationType="slide" onRequestClose={close}>
      <View style={styles.backdrop}>
        <View style={styles.sheet}>
          <Frame>
            <ScrollView contentContainerStyle={{ gap: 12 }}>
              <Text style={[styles.title, { color: colors.accentLight }]}>{target ? `Report ${target.label}` : ""}</Text>
              {!account ? (
                <>
                  <Text style={styles.body}>Log in to report or block.</Text>
                  <Button label="Log in" onPress={() => { close(); router.push("/sign-in"); }} />
                </>
              ) : done ? (
                <Text style={styles.body} accessibilityRole="alert">{done}</Text>
              ) : (
                <>
                  <Text style={styles.body}>Why are you reporting this?</Text>
                  {error ? <Notice>{error}</Notice> : null}
                  {REASONS.map(([value, label]) => (
                    <Pressable key={value} accessibilityRole="button" disabled={busy} onPress={() => { void report(value); }} style={[styles.reason, { borderColor: line }]}>
                      <Text style={styles.reasonText}>{label}</Text>
                    </Pressable>
                  ))}
                  {target?.username && target.username !== account.username ? (
                    <Button label={`Block ${target.displayName}`} kind="quiet" onPress={block} disabled={busy} />
                  ) : null}
                </>
              )}
              <Button label={done ? "Done" : "Cancel"} kind="quiet" onPress={close} />
            </ScrollView>
          </Frame>
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: "rgba(0,0,0,0.6)" },
  sheet: { maxHeight: "85%", backgroundColor: bg, padding: 12 },
  title: { fontFamily: "Cinzel_800ExtraBold", fontSize: 22 },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  reason: { minHeight: 44, justifyContent: "center", borderWidth: 1, paddingHorizontal: 12 },
  reasonText: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 15 },
});
