import { useCallback, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Switch, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../src/api";
import { useAuth } from "../src/auth";
import { useLoad } from "../src/load";
import type { PushSettings } from "../src/push";
import { forgetDevice, registerDevice, thisPhone, type PushState } from "../src/register-push";
import { Button, Frame, Notice, Screen, ink, inkMuted } from "../src/ui";

const KINDS: [string, string][] = [
  ["guild_application", "New guild applications"],
  ["guild_decision", "Guild application decisions"],
  ["guild_invite", "Invitations to apply to guilds"],
  ["squad_invite", "Co-stream invitations"],
  ["dm", "Direct messages"],
  ["light", "Lights"],
];

function phoneLine(phone: PushState | null, registered: boolean): string {
  if (!phone) return registered ? "This phone is registered." : "Checking this phone…";
  if (phone.state === "on") return "This phone is registered.";
  if (phone.state === "denied") return "Notifications are blocked for S.V.E.R in the phone's settings.";
  if (phone.state === "simulator") return "A simulator cannot register for push. Use a phone.";
  if (phone.state === "unconfigured") return "This build has no Expo project id yet, so it cannot register.";
  if (phone.state === "failed") return phone.message;
  return "This phone is not registered.";
}

export default function AlertsScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [settings, setSettings] = useState<PushSettings | null>(null);
  const [phone, setPhone] = useState<PushState | null>(null);
  const [registered, setRegistered] = useState(false);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!account) return;
    const result = await send<PushSettings>("GET", "/api/me/notifications/settings");
    if (result.ok) {
      setSettings(result.data);
      setMessage("");
    } else setMessage(result.error);
    setRegistered(await thisPhone());
  }, [account]);
  useLoad(load);

  async function save(next: PushSettings) {
    if (busy) return;
    setBusy(true);
    const result = await send<PushSettings>("PUT", "/api/me/notifications/settings", {
      site: next.site,
      push: next.push,
      email: next.email,
      community: next.community,
    });
    if (result.ok) {
      setSettings(result.data);
      setMessage("Saved.");
    } else setMessage(result.error);
    setBusy(false);
  }

  async function togglePhone() {
    if (busy) return;
    setBusy(true);
    setMessage("");
    if (registered) {
      await forgetDevice();
      setRegistered(false);
      setPhone({ state: "off" });
      await load();
    } else {
      const result = await registerDevice();
      setPhone(result);
      setRegistered(result.state === "on");
      if (result.state === "on") await load();
    }
    setBusy(false);
  }

  return (
    <Screen>
      <ScrollView contentContainerStyle={{ paddingTop: insets.top + 12, paddingBottom: insets.bottom + 24, paddingHorizontal: 16, gap: 16 }}>
        <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={() => router.back()} style={styles.back}>
          <Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Back</Text>
        </Pressable>
        <Frame>
          <Text style={[styles.title, { color: colors.accentLight }]}>Alerts</Text>
          {!account ? (
            <View style={styles.stack}>
              <Text style={styles.body}>Log in to choose which alerts reach this phone.</Text>
              <Button label="Log in" onPress={() => router.push("/sign-in")} />
            </View>
          ) : !settings ? (
            <Text style={styles.body}>{message || "Loading…"}</Text>
          ) : (
            <View style={styles.stack}>
              <Text style={styles.section}>Go-live</Text>
              <Text style={styles.body}>When a channel you follow goes live. At most once every six hours for each channel. This is the same push switch as the website, so turning it off stops push there too. To stop one channel, use the bell beside Following on the website.</Text>
              <Row label="Push when a channel you follow goes live" checked={settings.push} disabled={busy} onChange={value => { void save({ ...settings, push: value }); }} />
              <Text style={styles.section}>Other alerts</Text>
              <Text style={styles.body}>These use the same per-type push switch as the website. In-site notices that have no push switch are not sent to this phone.</Text>
              {KINDS.map(([kind, label]) => {
                const row = settings.community.find(item => item.kind === kind);
                if (!row) return null;
                return (
                  <Row
                    key={kind}
                    label={label}
                    checked={row.push}
                    disabled={busy}
                    onChange={value => {
                      void save({
                        ...settings,
                        community: settings.community.map(item => item.kind === kind ? { ...item, push: value } : item),
                      });
                    }}
                  />
                );
              })}
              <Text style={styles.section}>This phone</Text>
              <Text style={styles.body}>{phoneLine(phone, registered)}</Text>
              <Button
                label={busy ? "Please wait…" : registered ? "Turn off alerts on this phone" : "Turn on alerts on this phone"}
                kind="quiet"
                disabled={busy}
                onPress={() => { void togglePhone(); }}
              />
              {message ? <Notice>{message}</Notice> : null}
            </View>
          )}
        </Frame>
      </ScrollView>
    </Screen>
  );
}

function Row({ label, checked, disabled, onChange }: { label: string; checked: boolean; disabled: boolean; onChange: (value: boolean) => void }) {
  const { colors } = useAuth();
  return (
    <View style={styles.row}>
      <Text style={styles.rowLabel}>{label}</Text>
      <Switch
        accessibilityLabel={label}
        value={checked}
        disabled={disabled}
        onValueChange={onChange}
        trackColor={{ false: "#3A3D44", true: colors.accent }}
        thumbColor={ink}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  back: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
  stack: { gap: 14 },
  title: { fontFamily: "Cinzel_800ExtraBold", fontSize: 28, marginBottom: 8 },
  section: { fontFamily: "Cinzel_700Bold", color: ink, fontSize: 18 },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  row: { minHeight: 44, flexDirection: "row", alignItems: "center", gap: 12 },
  rowLabel: { flex: 1, color: ink, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 20 },
});
