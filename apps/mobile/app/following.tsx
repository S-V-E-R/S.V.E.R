import { useCallback, useState } from "react";
import { Pressable, RefreshControl, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../src/api";
import { useAuth } from "../src/auth";
import { factionColor } from "../src/brand";
import { useLoad } from "../src/load";
import { liveFirst } from "../src/streams";
import { Button, Frame, Notice, Screen, Tag, ink, inkMuted } from "../src/ui";

type Row = {
  user: { username: string | null; display_name: string; live?: boolean; faction?: string | null };
  followed_at: string;
  guilds?: { name: string; slug: string }[] | null;
};
type Page = { items: Row[]; next_cursor: string | null };

export default function FollowingScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [rows, setRows] = useState<Row[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  // ponytail: live-first sorts only the pages loaded so far (50 per page); a server-side live-first order is the upgrade if lists grow long.
  const load = useCallback(async (after?: string | null) => {
    const result = await send<Page>("GET", `/api/me/following${after ? `?cursor=${encodeURIComponent(after)}` : ""}`);
    setLoaded(true);
    if (!result.ok) { setError(result.error); return; }
    setError("");
    setRows(previous => (after ? [...previous, ...result.data.items] : result.data.items));
    setCursor(result.data.next_cursor);
  }, []);
  const first = useCallback(() => (account ? load() : Promise.resolve()), [account, load]);
  useLoad(first, 60000);

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 8, paddingBottom: insets.bottom + 24, paddingHorizontal: 16, gap: 16 }}
        refreshControl={<RefreshControl refreshing={refreshing} tintColor={colors.accent} onRefresh={() => { setRefreshing(true); void load().finally(() => setRefreshing(false)); }} />}
      >
        <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={() => router.back()} style={styles.back}>
          <Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Back</Text>
        </Pressable>
        <Text style={[styles.title, { color: colors.accentLight }]}>Following</Text>
        {!account ? <Notice>Log in to see the channels you follow.</Notice> : null}
        {error ? <Notice>{error}</Notice> : null}
        {account && loaded && rows.length === 0 && !error ? <Frame><Text style={styles.body}>Channels you follow appear here.</Text></Frame> : null}
        {liveFirst(rows).map(row => row.user.username ? (
          <Pressable key={row.user.username} accessibilityRole="button" accessibilityLabel={`${row.user.display_name}${row.user.live ? ", live" : ""}`}
            onPress={() => router.push(`/watch/${row.user.username}`)}>
            <Frame>
              <View style={styles.row}>
                <View style={[styles.mark, { backgroundColor: factionColor(row.user.faction) }]} />
                <View style={{ flex: 1, gap: 2 }}>
                  <Text style={styles.name}>{row.user.display_name}</Text>
                  <Text style={styles.body}>{row.guilds?.length ? `Via ${row.guilds.map(g => g.name).join(", ")}` : `@${row.user.username}`}</Text>
                </View>
                {row.user.live ? <Tag label="Live" tone="live" /> : null}
              </View>
            </Frame>
          </Pressable>
        ) : null)}
        {cursor ? <Button label="More" kind="quiet" onPress={() => { void load(cursor); }} /> : null}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  back: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
  title: { fontFamily: "Cinzel_800ExtraBold", fontSize: 28 },
  row: { flexDirection: "row", alignItems: "center", gap: 12 },
  // The channel's faction mark: the design system's diamond, in its faction color.
  mark: { width: 8, height: 8, transform: [{ rotate: "45deg" }] },
  name: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 16 },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 14 },
});
