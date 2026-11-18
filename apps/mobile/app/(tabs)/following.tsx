import { useCallback, useRef, useState } from "react";
import { Alert, FlatList, RefreshControl, StyleSheet, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { ChannelRow, StreamCard } from "../../src/cards";
import { ago } from "../../src/format";
import { useLoad } from "../../src/load";
import { liveFirst } from "../../src/streams";
import { bg, space } from "../../src/theme";
import { isHomeFeed, streamCards, type ChannelChip, type StreamCard as Card } from "../../src/types";
import { Button, CardSkeleton, EmptyState, Notice, PageHead, RowSkeleton, Screen, SectionHead } from "../../src/ui";

type Row = {
  user: ChannelChip & { live?: boolean; direct_follow?: boolean };
  followed_at: string;
  guilds?: { name: string; slug: string }[] | null;
};
type Page = { items: Row[]; next_cursor: string | null };

/** docs/DESIGN.md "Following": who's live as stream cards, then every followed channel, live first. */
export default function FollowingScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [rows, setRows] = useState<Row[]>([]);
  const [live, setLive] = useState<Card[]>([]);
  const [minute, setMinute] = useState(0);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [paging, setPaging] = useState(false);
  const pagingRef = useRef(false);

  // ponytail: live-first sorts only the pages loaded so far (50 per page); a server-side live-first order is the upgrade if lists grow long.
  const load = useCallback(async (after?: string | null) => {
    const result = await send<Page>("GET", `/api/me/following${after ? `?cursor=${encodeURIComponent(after)}` : ""}`);
    setLoaded(true);
    if (!result.ok) { setError(result.error); return; }
    setError("");
    setRows(previous => (after ? [...previous, ...result.data.items] : result.data.items));
    setCursor(result.data.next_cursor);
  }, []);
  const first = useCallback(async () => {
    if (!account) return;
    await Promise.all([
      load(),
      send<unknown>("GET", "/api/discovery/home").then(home => {
        // The live cards are a bonus over the list: a failed fetch keeps the last ones.
        if (!home.ok || !isHomeFeed(home.data)) return;
        setLive(streamCards(home.data.following));
        setMinute(Math.floor(Date.now() / 60000));
      }),
    ]);
  }, [account, load]);
  useLoad(first, 60000);

  async function more() {
    if (!cursor || pagingRef.current) return;
    pagingRef.current = true;
    setPaging(true);
    await load(cursor);
    pagingRef.current = false;
    setPaging(false);
  }

  function unfollow(username: string, name: string) {
    Alert.alert(`Unfollow ${name}?`, undefined, [
      { text: "Cancel", style: "cancel" },
      {
        text: "Unfollow",
        style: "destructive",
        onPress: () => {
          void send("DELETE", `/api/follows/${encodeURIComponent(username)}`).then(result => {
            if (!result.ok) { setError(result.error); return; }
            setRows(previous => previous.filter(row => row.user.username !== username));
            setLive(previous => previous.filter(stream => stream.username !== username));
          });
        },
      },
    ]);
  }

  const loading = !!account && !loaded;
  const empty = !!account && loaded && !error && rows.length === 0 && live.length === 0;
  const header = (
    <View style={styles.gap}>
      <PageHead eyebrow="Following" title="Following" />
      {!account ? <Notice>Log in to see the channels you follow.</Notice> : null}
      {error ? <Notice onRetry={() => { void first(); }}>{error}</Notice> : null}
      {empty ? <EmptyState title="You don't follow anyone yet." action={{ label: "Browse", onPress: () => router.push("/browse") }} /> : null}
      {loading ? <><CardSkeleton /><CardSkeleton /></> : null}
      {live.length ? <SectionHead title="Live" /> : null}
      {live.map(stream => <StreamCard key={stream.username} stream={stream} size="full" minute={minute} viewerFaction={account?.faction ?? null} />)}
      {loading || rows.length ? <SectionHead title="Channels" /> : null}
      {loading ? <><RowSkeleton /><RowSkeleton /><RowSkeleton /><RowSkeleton /></> : null}
    </View>
  );

  return (
    <Screen>
      <FlatList
        data={liveFirst(rows).filter(row => row.user.username)}
        keyExtractor={row => row.user.username ?? row.followed_at}
        ListHeaderComponent={header}
        ListFooterComponent={paging ? <RowSkeleton /> : undefined}
        onEndReached={() => { void more(); }}
        onEndReachedThreshold={0.5}
        contentContainerStyle={[styles.list, { paddingTop: insets.top + space.l }]}
        refreshControl={<RefreshControl refreshing={refreshing} tintColor={colors.accent} colors={[colors.accent]} progressBackgroundColor={bg} onRefresh={() => { setRefreshing(true); void first().finally(() => setRefreshing(false)); }} />}
        renderItem={({ item }) => {
          const username = item.user.username ?? "";
          const meta = item.guilds?.length ? `Via ${item.guilds.map(g => g.name).join(", ")}` : `@${username} · Followed ${ago(item.followed_at)}`;
          // The Unfollow button sits beside the row, not inside it, so screen readers reach it on its own.
          return (
            <View style={styles.row}>
              <View style={styles.grow}>
                <ChannelRow user={item.user} live={item.user.live} meta={meta} onPress={() => router.push(`/watch/${username}`)} />
              </View>
              {/* A guild follow is undone on the guild, not here. */}
              {item.user.direct_follow === false ? null : (
                <View><Button label="Unfollow" accessibilityLabel={`Unfollow ${item.user.display_name}`} tone="quiet" compact onPress={() => unfollow(username, item.user.display_name)} /></View>
              )}
            </View>
          );
        }}
      />
    </Screen>
  );
}

const styles = StyleSheet.create({
  list: { paddingHorizontal: space.l, paddingBottom: space.xl },
  gap: { gap: space.l },
  row: { flexDirection: "row", alignItems: "center", gap: space.m },
  grow: { flex: 1 },
});
