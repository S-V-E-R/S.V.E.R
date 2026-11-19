import { useCallback, useEffect, useState } from "react";
import { Alert, Modal, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "./api";
import { useAuth } from "./auth";
import { newId } from "./session";
import { Attempt, aboutValor } from "./spend";
import { scrim, space } from "./theme";
import { Button, Field, Notice, Panel, bg, inkMuted, type } from "./ui";
import { GetValorSheet } from "./valor";

type Tier = { tier: number; cents: number; valor: number };
type Mine = { tier: number; paid_through: string; months: number; auto_renew: boolean; card: boolean };
type Status = { can_subscribe: boolean; tiers: Tier[]; gift_counts: number[]; mine: Mine | null };
const date = (iso: string) => new Date(iso).toLocaleDateString();

/**
 * The Watch streamer bar's Subscribe button and its sheet (docs/SUPPORT.md "Subscriptions"). In the
 * app a month or a gift is paid only in Valor; card subscriptions, keeping one by card and upgrades
 * charge a card for a digital good, so they stay on the website. Cancel renewal is offered, since it
 * only stops a charge.
 */
export function SubscribeButton({ username }: { username: string }) {
  const { account, colors } = useAuth();
  const insets = useSafeAreaInsets();
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [status, setStatus] = useState<Status | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [gift, setGift] = useState({ tier: 1, count: 1, recipient: "" });
  const [getValor, setGetValor] = useState(false);
  const [subAttempt] = useState(() => new Attempt(newId));
  const [giftAttempt] = useState(() => new Attempt(newId));

  const load = useCallback(async () => {
    const result = await send<Status>("GET", `${path}/subscription`);
    if (result.ok && Array.isArray(result.data.tiers) && result.data.tiers.length) setStatus(result.data);
  }, [path]);
  useEffect(() => {
    // The status arrives after the request; the call itself is not a render update.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load();
  }, [load, account?.id]);
  // The Valor path needs no card processor, so `available` is not checked.
  if (!status?.can_subscribe) return null;

  async function act(route: string, body: unknown, done: string, attempt?: Attempt) {
    setBusy(true); setError(""); setMessage("");
    const result = await send("POST", `${path}${route}`, body);
    attempt?.settle(result);
    setBusy(false);
    if (result.ok) { setMessage(done); await load(); return; }
    setError(result.error);
  }
  function ask(title: string, action: string, run: () => void) {
    Alert.alert(title, undefined, [{ text: "Cancel", style: "cancel" }, { text: action, onPress: run }]);
  }
  const subscribe = (t: Tier) => ask(`Subscribe at Tier ${t.tier} for ${t.valor.toLocaleString()} Valor?`, "Subscribe", () => {
    void act("/subscription", { tier: t.tier, pay: "valor", id: subAttempt.take(`${t.tier}`) }, "Subscribed for a month. Thank you!", subAttempt);
  });
  const cancel = () => ask("Stop renewing? Your benefits run to the end of the paid month.", "Stop renewing", () => {
    void act("/subscription/cancel", undefined, "Auto-renewal is off.");
  });
  const price = status.tiers.find(t => t.tier === gift.tier) ?? status.tiers[0]!;
  const recipient = gift.count === 1 ? gift.recipient.trim() : undefined;
  const total = price.valor * gift.count;
  const sendGift = () => ask(`Gift ${gift.count} Tier ${price.tier} ${gift.count === 1 ? "sub" : "subs"} for ${total.toLocaleString()} Valor?`, "Gift", () => {
    const id = giftAttempt.take(`${price.tier}:${gift.count}:${recipient ?? ""}`);
    void act("/gifts", { tier: price.tier, count: gift.count, recipient, pay: "valor", id }, "Gift sent. Thank you!", giftAttempt);
  });
  const mine = status.mine;
  const close = () => { setOpen(false); setMessage(""); setError(""); };

  return (
    <>
      <Button compact label={mine ? `Subscribed · Tier ${mine.tier}` : "Subscribe"} tone={mine ? "quiet" : "primary"} onPress={() => (account ? setOpen(true) : router.push("/sign-in"))} />
      <Modal visible={open} transparent animationType="slide" onRequestClose={close} supportedOrientations={["portrait", "landscape"]}>
        <View style={styles.backdrop}>
          <Panel style={{ ...styles.sheet, paddingBottom: insets.bottom + space.m }}>
            <ScrollView contentContainerStyle={styles.gap} keyboardShouldPersistTaps="handled">
              {mine ? (
                <View style={styles.gap}>
                  <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">Your subscription</Text>
                  <Text style={type.body}>Tier {mine.tier} · {mine.months} {mine.months === 1 ? "month" : "months"} · {mine.auto_renew ? `renews ${date(mine.paid_through)}` : `ends ${date(mine.paid_through)}`}</Text>
                  {mine.card && mine.auto_renew ? <Button label="Cancel renewal" tone="quiet" disabled={busy} onPress={cancel} /> : null}
                  {!mine.card ? status.tiers.map(t => (
                    <Button key={t.tier} label={`Add a Tier ${t.tier} month for ${t.valor.toLocaleString()} Valor`} tone="quiet" disabled={busy} onPress={() => subscribe(t)} />
                  )) : null}
                </View>
              ) : (
                <View style={styles.gap}>
                  <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">Subscribe to {username}</Text>
                  <Text style={styles.muted}>A subscriber badge, subscriber emotes and subscriber-only chat.</Text>
                  {status.tiers.map(t => (
                    <View key={t.tier} style={styles.tier}>
                      <Text style={type.strong}>Tier {t.tier}</Text>
                      <Button label={`${t.valor.toLocaleString()} Valor for one month`} tone="quiet" disabled={busy} onPress={() => subscribe(t)} />
                      <Text style={type.small}>Valor months never renew.</Text>
                    </View>
                  ))}
                </View>
              )}
              <View style={styles.gap}>
                <Text style={[type.title, { color: colors.accentLight }]} accessibilityRole="header">Gift subs</Text>
                <Text style={styles.muted}>A gifted month never renews. Random gifts go to recent chatters who allow gifts.</Text>
                <Text style={type.label}>Tier</Text>
                <View style={styles.choices}>
                  {status.tiers.map(t => (
                    <Button key={t.tier} compact label={`Tier ${t.tier}`} tone="quiet" selected={t.tier === price.tier} onPress={() => setGift(g => ({ ...g, tier: t.tier }))} />
                  ))}
                </View>
                <Text style={type.label}>How many</Text>
                <View style={styles.choices}>
                  {(status.gift_counts ?? []).map(n => (
                    <Button key={n} compact label={n === 1 ? "1 to a viewer" : `${n} to random chatters`} tone="quiet" selected={n === gift.count} onPress={() => setGift(g => ({ ...g, count: n }))} />
                  ))}
                </View>
                {gift.count === 1 ? (
                  <Field label="Username" value={gift.recipient} maxLength={26} autoCapitalize="none" autoCorrect={false} onChangeText={recipient => setGift(g => ({ ...g, recipient }))} />
                ) : null}
                <Button label={`Gift with ${total.toLocaleString()} Valor`} disabled={busy} onPress={sendGift} />
              </View>
              {message ? <Text style={type.body} accessibilityRole="alert">{message}</Text> : null}
              {error ? <Notice>{error}</Notice> : null}
              {aboutValor(error) ? <Button label="Get Valor" tone="quiet" compact onPress={() => setGetValor(true)} /> : null}
              <Button label="Close" tone="quiet" onPress={close} />
            </ScrollView>
          </Panel>
          {/* Inside this Modal so it stacks over the sheet on iOS. */}
          <GetValorSheet visible={getValor} onClose={() => setGetValor(false)} />
        </View>
      </Modal>
    </>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: scrim },
  sheet: { maxHeight: "85%", backgroundColor: bg, borderWidth: 0, borderTopWidth: 1, padding: space.m },
  gap: { gap: space.m },
  muted: { ...type.body, color: inkMuted },
  tier: { gap: space.s },
  choices: { flexDirection: "row", flexWrap: "wrap", gap: space.s },
});
