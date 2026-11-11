import { useState } from "react";
import { Linking, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useAuth } from "../src/auth";
import { factionLabel } from "../src/brand";
import { Button, Field, Frame, Notice, Screen, inkMuted } from "../src/ui";
import { webOrigin } from "../src/urls";

// Signup (Turnstile), password recovery and account deletion stay on the website (docs/MOBILE.md).
const openSite = (path: string) => { void Linking.openURL(`${webOrigin}${path}`); };

export default function SignInScreen() {
  const insets = useSafeAreaInsets();
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
    if (result.data.signed_in) router.back();
  }

  async function out() {
    setBusy(true);
    await signOut();
    setBusy(false);
  }

  return (
    <Screen>
      <ScrollView contentContainerStyle={{ paddingTop: insets.top + 12, paddingBottom: insets.bottom + 24, paddingHorizontal: 16, gap: 16 }}>
        <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={() => router.back()} style={styles.back}>
          <Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Back</Text>
        </Pressable>
        <Frame>
          {account ? (
            <View style={styles.stack}>
              <Text style={[styles.title, { color: colors.accentLight }]}>{account.username}</Text>
              <Text style={styles.body}>{factionLabel(account.faction) ?? "No faction yet"}</Text>
              {account.deletion_due ? <Notice>This account is pending deletion.</Notice> : null}
              {!account.email_verified ? <Text style={styles.body}>Confirm your email to chat and go live. You can browse and watch now.</Text> : null}
              <Button label="Alerts" kind="quiet" onPress={() => router.push("/alerts")} />
              <Button label={busy ? "Please wait…" : "Log out"} kind="quiet" onPress={() => { void out(); }} disabled={busy} />
              <Button label="Delete account" kind="quiet" onPress={() => openSite("/account")} />
            </View>
          ) : (
            <View style={styles.stack}>
              <Text style={[styles.title, { color: colors.accentLight }]}>{needsCode ? "Authenticator" : "Log in"}</Text>
              <Text style={styles.body}>{needsCode ? "Enter the code from your authenticator, or a recovery code." : "Email or username, and your password."}</Text>
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
                <Button label="Create account" kind="quiet" onPress={() => openSite("/signup")} />
                <Button label="Forgot password" kind="quiet" onPress={() => openSite("/forgot")} />
                <Text style={styles.body}>Signed up with Google, Twitch, Discord or Kick? Use Forgot password on the website to add a password, then log in here.</Text>
              </>}
            </View>
          )}
        </Frame>
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  back: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
  stack: { gap: 16 },
  title: { fontFamily: "Cinzel_800ExtraBold", fontSize: 28 },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
});
