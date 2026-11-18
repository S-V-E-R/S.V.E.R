import { useCallback, useState } from "react";
import { StyleSheet, Switch, Text, View } from "react-native";
import { router } from "expo-router";
import { send } from "../src/api";
import { useAuth } from "../src/auth";
import { useLoad } from "../src/load";
import type { PushSettings } from "../src/push";
import { forgetDevice, registerDevice, thisPhone, type PushState } from "../src/register-push";
import { space } from "../src/theme";
import { Button, EmptyState, HeaderBar, Notice, Page, Panel, RowSkeleton, hexAlpha, ink, type } from "../src/ui";

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
  if (phone.state === "expo-go") return "Expo Go on Android cannot receive push. Use a development or store build.";
  if (phone.state === "unconfigured") return "This build has no Expo project id yet, so it cannot register.";
  if (phone.state === "failed") return phone.message;
  return "This phone is not registered.";
}

export default function AlertsScreen() {
  const { account } = useAuth();
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
    <Page header={<HeaderBar title="Alerts" onBack={() => router.back()} />}>
      {!account ? (
        <EmptyState title="Log in for alerts" body="Log in to choose which alerts reach this phone." action={{ label: "Log in", onPress: () => router.push("/sign-in") }} />
      ) : !settings ? (
        message ? <Notice onRetry={() => { void load(); }}>{message}</Notice> : <><RowSkeleton /><RowSkeleton /><RowSkeleton /><RowSkeleton /></>
      ) : (
        <>
          <Text style={type.title}>Go-live</Text>
          <Panel>
            <Row
              label="Push when a channel you follow goes live"
              hint="At most once every six hours for each channel. This is the same push switch as the website, so turning it off stops push there too. To stop one channel, use the bell beside Following on the website."
              checked={settings.push}
              disabled={busy}
              onChange={value => { void save({ ...settings, push: value }); }}
            />
          </Panel>
          <Text style={type.title}>Other alerts</Text>
          <Text style={type.small}>These use the same per-type push switch as the website. In-site notices that have no push switch are not sent to this phone.</Text>
          <Panel>
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
          </Panel>
          <Text style={type.title}>This phone</Text>
          <Panel style={styles.stack}>
            <Text style={type.small}>{phoneLine(phone, registered)}</Text>
            <Button
              label={busy ? "Please wait…" : registered ? "Turn off alerts on this phone" : "Turn on alerts on this phone"}
              tone="quiet"
              disabled={busy}
              onPress={() => { void togglePhone(); }}
            />
          </Panel>
          {message ? <Notice>{message}</Notice> : null}
        </>
      )}
    </Page>
  );
}

function Row({ label, hint, checked, disabled, onChange }: { label: string; hint?: string; checked: boolean; disabled: boolean; onChange: (value: boolean) => void }) {
  const { colors } = useAuth();
  return (
    <View style={styles.row}>
      <View style={styles.grow}>
        <Text style={type.strong}>{label}</Text>
        {hint ? <Text style={type.small}>{hint}</Text> : null}
      </View>
      <Switch
        accessibilityLabel={label}
        accessibilityHint={hint}
        value={checked}
        disabled={disabled}
        onValueChange={onChange}
        trackColor={{ false: hexAlpha(ink, 0.2), true: colors.accent }}
        thumbColor={ink}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  stack: { gap: space.m },
  row: { minHeight: 56, flexDirection: "row", alignItems: "center", gap: space.m, paddingVertical: space.s },
  grow: { flex: 1, gap: space.xs },
});
