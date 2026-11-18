import { useState } from "react";
import { Alert, Modal, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "./api";
import { useAuth } from "./auth";
import { line, scrim, space } from "./theme";
import { Button, Notice, Panel, bg, ink, inkMuted, type } from "./ui";

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
  const insets = useSafeAreaInsets();
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

  return (
    <Modal visible={!!target} transparent animationType="slide" onRequestClose={close} supportedOrientations={["portrait", "landscape"]}>
      <View style={styles.backdrop}>
        {/* A sheet, not a bracketed slot: a plain panel with only its top edge drawn. */}
        <Panel style={{ ...styles.sheet, paddingBottom: insets.bottom + space.m }}>
          <ScrollView contentContainerStyle={{ gap: space.m }}>
            <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">{target ? `Report ${target.label}` : ""}</Text>
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
                  <Pressable key={value} accessibilityRole="button" disabled={busy} onPress={() => { void report(value); }} style={[styles.reason, { borderColor: line(colors.accent) }]}>
                    <Text style={styles.reasonText}>{label}</Text>
                  </Pressable>
                ))}
                {target?.username && target.username !== account.username ? (
                  <Button label={`Block ${target.displayName}`} tone="danger" onPress={block} disabled={busy} />
                ) : null}
              </>
            )}
            <Button label={done ? "Done" : "Cancel"} tone="quiet" onPress={close} />
          </ScrollView>
        </Panel>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: scrim },
  sheet: { maxHeight: "85%", backgroundColor: bg, borderWidth: 0, borderTopWidth: 1, padding: space.m },
  body: { ...type.body, color: inkMuted },
  reason: { minHeight: 44, justifyContent: "center", borderWidth: 1, paddingHorizontal: space.m },
  reasonText: { ...type.strong, color: ink },
});
