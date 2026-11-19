import { useCallback, useEffect, useState } from "react";
import { Alert, Linking, Modal, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, View } from "react-native";
import { isRunningInExpoGo } from "expo";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import type * as Iap from "expo-iap";
import { send } from "./api";
import { useAuth } from "./auth";
import { claimMessage, claimRequest, dollars, mergePacks, packValor, shouldFinish, showsWebLink, type Claim, type Os, type StorePack, type Wallet } from "./store";
import { line, scrim, space } from "./theme";
import { Button, Notice, Panel, SectionHead, Skeleton, bg, hexAlpha, ink, inkMuted, type } from "./ui";
import { webOrigin } from "./urls";

type Module = typeof Iap;

const os: Os | null = Platform.OS === "ios" || Platform.OS === "android" ? Platform.OS : null;
// Expo Go has no store module and expo-iap throws when it loads there, so it is only ever loaded
// with import() where it can work. Spending Valor works everywhere.
const native = os !== null && !isRunningInExpoGo();
const store = os === "ios" ? "apple" : "google";

const LOCKED = "A payment was refunded or disputed after its Valor was spent, so spending is locked until the balance is back above zero.";
const PAYING = "Waiting for the payment to finish. Your Valor appears here once it's paid.";
const APPROVAL = "Waiting for approval. Your Valor appears here once the purchase goes through.";

let loaded: Promise<Module> | null = null;
const iap = () => (loaded ??= import("expo-iap"));

/** Android: whether the connection has Google's external content links program turned on. */
let program = false;
async function connect(m: Module, withProgram = program) {
  if (withProgram !== program) {
    program = withProgram;
    await m.endConnection().catch(() => false);
  }
  await m.initConnection(program ? { enableBillingProgramAndroid: "external-content-link" } : undefined);
}

/** True from a purchase started on this screen until its first outcome. Launch replays stay quiet unless they credit. */
let buying = false;
/** Wallets on screen reload when a purchase is credited. */
const credited = new Set<() => void>();

function say(message: string) {
  buying = false;
  Alert.alert(message);
}

function storeError(m: Module, error: unknown) {
  const code = (error as { code?: unknown } | null)?.code;
  if (code === m.ErrorCode.UserCancelled) buying = false;
  if (code === m.ErrorCode.AlreadyOwned || code === m.ErrorCode.DuplicatePurchase) {
    // An earlier purchase of this pack isn't credited or consumed yet: claim it now, which also
    // clears it so the pack can be bought again.
    void recover(m)
      .then(found => { if (!found && buying && code === m.ErrorCode.AlreadyOwned) say(m.getUserFriendlyErrorMessage({ code })); })
      .catch(() => {})
      .finally(() => { buying = false; });
    return;
  }
  if (!buying || code === m.ErrorCode.UserCancelled) return;
  if (code === m.ErrorCode.DeferredPayment) say(APPROVAL);
  else if (code === m.ErrorCode.Pending) say(PAYING);
  else say(m.getUserFriendlyErrorMessage({ code: typeof code === "string" ? code : undefined }));
}

const claiming = new Set<string>();

/** Credit one store purchase through the server, then finish it (iOS) only on credited or void. Android: the server consumes. */
async function claim(m: Module, purchase: Iap.Purchase) {
  if (!os) return;
  if (os === "android" && purchase.purchaseState === "pending") {
    if (buying) say(PAYING);
    return;
  }
  const request = claimRequest(os, purchase);
  if (!request) return;
  const key = JSON.stringify(request.body);
  if (claiming.has(key)) return;
  claiming.add(key);
  try {
    const result = await send<Claim>("POST", request.path, request.body);
    if (os === "ios" && shouldFinish(result)) await m.finishTransaction({ purchase, isConsumable: true }).catch(() => false);
    if (result.ok && result.data.status === "credited") credited.forEach(reload => reload());
    if (result.ok && result.data.status !== "credited") {
      if (buying && result.data.status === "pending") say(PAYING);
      else buying = false;
      return;
    }
    const each = packValor(purchase.productId);
    const message = claimMessage(result, each ? each * Math.max(1, purchase.quantity) : null);
    if (message && (buying || result.ok)) say(message);
  } finally {
    claiming.delete(key);
  }
}

/** Claim every purchase the store still holds. Returns whether there were any. */
async function recover(m: Module, live = () => true) {
  const waiting: Iap.Purchase[] = os === "ios" ? await m.getPendingTransactionsIOS() : await m.getAvailablePurchases();
  for (const purchase of waiting) if (live()) await claim(m, purchase);
  return waiting.length > 0;
}

/** Open the store's sheet. The outcome arrives through StoreSync's listeners. */
async function startPurchase(product: string, token: string) {
  const m = await iap();
  buying = true;
  try {
    await m.requestPurchase({
      type: "in-app",
      request: {
        apple: { sku: product, appAccountToken: token },
        google: { skus: [product], obfuscatedAccountId: token },
      },
    });
  } catch (e) {
    storeError(m, e);
  }
}

/**
 * Mounted once while signed in: listens for store purchases and claims the ones the store still
 * holds (interrupted, approved later, or not yet credited), so nothing paid is left behind.
 */
export function StoreSync() {
  const { account } = useAuth();
  const id = account?.id;
  useEffect(() => {
    if (!id || !native) return;
    let gone = false;
    const subs: { remove: () => void }[] = [];
    void (async () => {
      const m = await iap();
      if (gone) return;
      subs.push(
        m.purchaseUpdatedListener(purchase => { void claim(m, purchase); }),
        m.purchaseErrorListener(error => storeError(m, error)),
      );
      await connect(m);
      await recover(m, () => !gone);
    })().catch(() => {});
    return () => {
      gone = true;
      subs.forEach(sub => sub.remove());
    };
  }, [id]);
  return null;
}

/** The balance, the locked notice and the store's Valor packs. Prices always come from the store. */
export function BuyValor() {
  const { account, colors } = useAuth();
  const [wallet, setWallet] = useState<Wallet | null>(null);
  const [packs, setPacks] = useState<(StorePack & { price: string })[] | null>(null);
  const [link, setLink] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [consent, setConsent] = useState(false);
  const [askConsent, setAskConsent] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    const result = await send<Wallet>("GET", "/api/me/wallet");
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setError("");
    setWallet(result.data);
    const sold = result.data.store;
    if (!os || !native || !sold?.[store]) return;
    try {
      const m = await iap();
      const googleFlag = os === "android" && sold.google_us_link === true;
      await connect(m, googleFlag);
      const products = await m.fetchProducts({ skus: sold.packs.map(p => p.product), type: "in-app" });
      setPacks(mergePacks(sold.packs, products ?? []));
      const country = await m.getStorefront().catch(() => null);
      const available = googleFlag && country === "US"
        ? (await m.isBillingProgramAvailableAndroid("external-content-link").catch(() => null))?.isAvailable === true
        : false;
      setLink(showsWebLink(os, country, googleFlag, available));
    } catch {
      // No products in this storefront, no store connection, or the sideloaded test build.
      setPacks([]);
    }
  }, []);

  useEffect(() => {
    if (!account) return;
    const reload = () => { void load(); };
    reload();
    credited.add(reload);
    return () => { credited.delete(reload); };
  }, [account, load]);

  async function buy(product: string) {
    setBusy(true);
    setMessage("");
    // The server's purchase check (the under-18 cap and guardian consent) hands out the account token.
    const pre = await send<{ token: string }>("POST", "/api/me/wallet/store", { store, product, guardian_consent: consent });
    if (!pre.ok) {
      if (pre.field === "guardian_consent") setAskConsent(true);
      setMessage(pre.error);
      setBusy(false);
      return;
    }
    await startPurchase(product, pre.data.token);
    setBusy(false);
  }

  async function openWeb() {
    const url = `${webOrigin}/wallet`;
    if (os !== "android") {
      void Linking.openURL(url);
      return;
    }
    // Google shows its own information screen before the browser opens.
    const m = await iap();
    await m.launchExternalLinkAndroid({ billingProgram: "external-content-link", launchMode: "launch-in-external-browser-or-app", linkType: "link-to-digital-content-offer", linkUri: url }).catch(() => false);
  }

  if (!account) return <Text style={type.body}>Log in to see your Valor.</Text>;
  if (!wallet) return error ? <Notice onRetry={() => { void load(); }}>{error}</Notice> : <Skeleton w="100%" h={160} />;

  const where = os === "ios" ? "the App Store" : "Google Play";
  const needsConsent = (wallet.minor && !wallet.guardian_confirmed) || askConsent;
  let shop;
  if (!native) shop = <Text style={type.body}>Buying Valor needs the App Store or Google Play version of the app.</Text>;
  else if (!wallet.store?.[store]) shop = <Text style={type.body}>Buying Valor in the app isn&apos;t available yet.</Text>;
  else if (!packs) shop = <Skeleton w="100%" h={168} />;
  else if (packs.length === 0) shop = <Notice onRetry={() => { void load(); }}>Valor packs couldn&apos;t load. Try again.</Notice>;
  else {
    shop = (
      <>
        <Text style={type.body}>Paid through {where}.</Text>
        {wallet.minor && wallet.cap_cents !== null ? (
          <Text style={type.small}>Accounts under 18 can spend up to {dollars(wallet.cap_cents)} a month ({dollars(wallet.month_cents ?? 0)} so far this month).</Text>
        ) : null}
        {needsConsent ? (
          <View style={styles.consent}>
            <Text style={[type.small, styles.grow]}>I&apos;m this account holder&apos;s parent or legal guardian, the store account on this device is mine, and I consent to this purchase.</Text>
            <Switch
              accessibilityLabel="Parent or guardian consent"
              value={consent}
              onValueChange={setConsent}
              trackColor={{ false: hexAlpha(ink, 0.2), true: colors.accent }}
              thumbColor={ink}
            />
          </View>
        ) : null}
        {message ? <Notice>{message}</Notice> : null}
        <View>
          {packs.map(pack => {
            const label = `${pack.valor.toLocaleString("en-US")} Valor`;
            const disabled = busy || (needsConsent && !consent);
            return (
              <Pressable
                key={pack.product}
                accessibilityRole="button"
                accessibilityLabel={`${label}, ${pack.price}`}
                accessibilityState={{ disabled }}
                disabled={disabled}
                onPress={() => { void buy(pack.product); }}
                style={({ pressed }) => [styles.pack, { borderColor: line(colors.accent) }, pressed && styles.pressed, disabled && styles.disabled]}
              >
                <Text style={[type.strong, styles.grow]}>{label}</Text>
                <Text style={[type.strong, { color: colors.accentLight }]}>{pack.price}</Text>
              </Pressable>
            );
          })}
        </View>
        {link ? (
          <Pressable accessibilityRole="link" onPress={() => { void openWeb(); }} style={styles.link}>
            <Text style={[type.small, styles.underline]}>Bonus Valor packs are on sver.tv.</Text>
          </Pressable>
        ) : null}
        <Text style={type.small}>
          Purchases are non-refundable except where the law or the refund policy says otherwise. Refunds for purchases in the app are handled by {os === "ios" ? "Apple" : "Google Play"}. A refunded purchase removes its Valor.
        </Text>
      </>
    );
  }

  return (
    <View style={styles.stack}>
      <SectionHead title="Your Valor" />
      <Text style={[type.display, { color: colors.accentLight }]}>{wallet.valor.toLocaleString("en-US")} Valor</Text>
      {wallet.locked ? <Notice>{LOCKED}</Notice> : null}
      <Text style={[type.body, styles.muted]}>Pay tribute in a streamer&apos;s chat, play Skills and subscribe. The streamer earns 0.8¢ for every Valor. Valor never expires, can&apos;t be transferred and can&apos;t be cashed out.</Text>
      <SectionHead title="Get Valor" />
      {shop}
    </View>
  );
}

/** Buy Valor without leaving the stream: the tribute composer, Skills and Subscribe open it from "Get Valor". */
export function GetValorSheet({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  const insets = useSafeAreaInsets();
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose} supportedOrientations={["portrait", "landscape"]}>
      <View style={styles.backdrop}>
        <Panel style={{ ...styles.sheet, paddingBottom: insets.bottom + space.m }}>
          <ScrollView contentContainerStyle={styles.stack}>
            {visible ? <BuyValor /> : null}
            <Button label="Done" tone="quiet" onPress={onClose} />
          </ScrollView>
        </Panel>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  stack: { gap: space.m },
  grow: { flex: 1 },
  muted: { color: inkMuted },
  consent: { flexDirection: "row", alignItems: "center", gap: space.m },
  pack: { minHeight: 56, flexDirection: "row", alignItems: "center", gap: space.m, borderBottomWidth: 1 },
  pressed: { opacity: 0.85 },
  disabled: { opacity: 0.5 },
  link: { minHeight: 44, justifyContent: "center" },
  underline: { textDecorationLine: "underline" },
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: scrim },
  sheet: { maxHeight: "85%", backgroundColor: bg, borderWidth: 0, borderTopWidth: 1, padding: space.m },
});
