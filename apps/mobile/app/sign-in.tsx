import { useState } from "react";
import { Alert, Image, Linking, Pressable, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { factionInfo, isFaction } from "../../web/lib/factions";
import { useAuth } from "../src/auth";
import { ArrowIcon, CloseIcon } from "../src/icons";
import { line, space } from "../src/theme";
import { Button, Field, Frame, HeaderBar, IconButton, Notice, Page, inkMuted, type } from "../src/ui";
import { crestUrl, webOrigin } from "../src/urls";

// Signup (Turnstile), password recovery, settings and account deletion stay on the website (docs/MOBILE.md).
const openSite = (path: string) => { void Linking.openURL(`${webOrigin}${path}`); };

export default function SignInScreen() {
  return <Account />;
}

/** Log in, or the signed-in account. As a modal it closes on success; as the Account tab it stays put. */
export function Account({ tab = false }: { tab?: boolean }) {
  const { account, colors, signIn, confirmMfa, signOut } = useAuth();
  const [identifier, setIdentifier] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [needsCode, setNeedsCode] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit() {
    setBusy(true);
    setError("");
    const result = needsCode ? await confirmMfa(code.trim()) : await signIn(identifier.trim(), password);
    setBusy(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    if (result.data.requires_mfa) {
      setNeedsCode(true);
      return;
    }
    if (result.data.signed_in && !tab) router.back();
  }

  function logOut() {
    Alert.alert("Log out?", "You can log back in any time.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Log out", style: "destructive", onPress: () => {
          setBusy(true);
          void signOut().finally(() => setBusy(false));
        },
      },
    ]);
  }

  const header = tab ? undefined : <HeaderBar title={account ? "Account" : "Log in"} right={<IconButton icon={c => <CloseIcon color={c} />} label="Close" onPress={() => router.back()} />} />;

  if (account) {
    const faction = isFaction(account.faction) ? factionInfo(account.faction) : null;
    return (
      <Page tabbed={tab} header={header} head={{ eyebrow: "Account", title: account.username }}>
        <Frame style={styles.stack}>
          <View style={styles.faction}>
            {faction ? <Image source={{ uri: crestUrl(faction.slug) }} style={styles.crest} accessibilityIgnoresInvertColors /> : null}
            <Text style={[type.strong, styles.grow]}>{faction ? `${faction.name} · ${faction.title}` : "No faction yet"}</Text>
          </View>
          {account.deletion_due ? <Notice>This account is pending deletion.</Notice> : null}
          {!account.email_verified ? <Text style={[type.body, styles.muted]}>Confirm your email to chat and go live. You can browse and watch now.</Text> : null}
        </Frame>
        <View>
          <Row label="Alerts" onPress={() => router.push("/alerts")} />
          <Row label="Settings on sver.tv" link onPress={() => openSite("/settings")} />
        </View>
        <Button label="Delete account" tone="danger" onPress={() => openSite("/account")} />
        <Button label={busy ? "Please wait…" : "Log out"} tone="quiet" onPress={logOut} disabled={busy} />
      </Page>
    );
  }

  return (
    <Page tabbed={tab} header={header}>
      <Frame style={styles.stack}>
        <Text style={[type.logo, { color: colors.accentLight }]} accessibilityRole="header">{needsCode ? "Authenticator" : "Log in"}</Text>
        <Text style={[type.body, styles.muted]}>{needsCode ? "Enter the code from your authenticator, or a recovery code." : "Welcome back."}</Text>
        {error ? <Notice>{error}</Notice> : null}
        {needsCode ? (
          <Field label="Code" value={code} onChangeText={setCode} autoCapitalize="none" autoCorrect={false} keyboardType="number-pad" />
        ) : (
          <>
            <Field label="Email or username" value={identifier} onChangeText={setIdentifier} autoCapitalize="none" autoCorrect={false} autoComplete="username" />
            <Field label="Password" value={password} onChangeText={setPassword} secureTextEntry autoCapitalize="none" autoComplete="current-password" />
          </>
        )}
        <Button label={busy ? "Please wait…" : needsCode ? "Confirm" : "Log in"} onPress={() => { void submit(); }} disabled={busy} />
        {needsCode ? null : <>
          <Button label="Create account" tone="quiet" onPress={() => openSite("/signup")} />
          <Button label="Forgot password" tone="quiet" onPress={() => openSite("/forgot")} />
          <Text style={type.small}>Signed up with Google, Twitch, Discord or Kick? Use Forgot password on the website to add a password, then log in here.</Text>
        </>}
      </Frame>
    </Page>
  );
}

/** A 56dp list row that opens another screen, or the website when `link`. */
function Row({ label, link, onPress }: { label: string; link?: boolean; onPress: () => void }) {
  const { colors } = useAuth();
  return (
    <Pressable accessibilityRole={link ? "link" : "button"} accessibilityLabel={label} onPress={onPress} style={({ pressed }) => [styles.row, { borderColor: line(colors.accent) }, pressed && styles.pressed]}>
      <Text style={[type.strong, styles.grow]}>{label}</Text>
      <ArrowIcon color={inkMuted} />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  stack: { gap: space.l },
  faction: { flexDirection: "row", alignItems: "center", gap: space.m },
  crest: { width: 56, height: 56 },
  grow: { flex: 1 },
  muted: { color: inkMuted },
  row: { minHeight: 56, flexDirection: "row", alignItems: "center", gap: space.m, borderBottomWidth: 1 },
  pressed: { opacity: 0.85 },
});
