import { useCallback, useState, type ReactNode } from "react";
import { ScrollView, StyleSheet, Text, View } from "react-native";
import { router, useFocusEffect } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { factionColor } from "../../src/brand";
import { ChannelRow, StreamCard } from "../../src/cards";
import { ago } from "../../src/format";
import { SearchIcon } from "../../src/icons";
import { rotationOrder } from "../../src/streams";
import { space } from "../../src/theme";
import { isHomeFeed, isStreamCard, streamCards, type MagnetLane, type RecentChannel, type Spotlight, type StreamCard as Card } from "../../src/types";
import { Button, CardSkeleton, EmptyState, Frame, IconButton, MagnetMark, Notice, Page, SectionHead, Tag, type } from "../../src/ui";

type Spot = Spotlight & { stream: Card };
type Recent = RecentChannel & { user: { username: string } };
type Feed = { live: Card[]; following: Card[]; spotlights: Spot[]; recent: Recent[] };

export default function MagnetScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [home, setHome] = useState<Feed | null>(null);
  const [featured, setFeatured] = useState<MagnetLane["featured"]>(null);
  const [minute, setMinute] = useState(0);
  const [error, setError] = useState("");
  // No empty state before the first answer arrives.
  const [loaded, setLoaded] = useState(false);
  const viewerFaction = account?.faction ?? null;

  /** Loads the feed and returns when MAGNet next rotates (ms), if known. */
  const load = useCallback(async (): Promise<number | null> => {
    const [res, lane] = await Promise.all([
      send<unknown>("GET", "/api/discovery/home"),
      send<MagnetLane>("GET", "/api/magnet/global"),
    ]);
    setLoaded(true);
    if (!res.ok || !isHomeFeed(res.data)) {
      setError(res.ok ? "Live channels couldn’t be loaded." : res.error);
      return null;
    }
    const data = res.data;
    setHome({
      // The API's fair rotation; never re-sorted here (docs/MAGNET.md).
      live: rotationOrder(streamCards(data.live)),
      following: streamCards(data.following),
      // Staff picks have no stream while the channel is offline.
      spotlights: data.spotlights.filter(isSpot),
      recent: data.recent.filter(isRecent),
    });
    // A live still changes under one URL and the Android image cache keeps the first one forever,
    // so the cards ask for it under a per-minute key, like the website's LiveThumbnail.
    setMinute(Math.floor(Date.now() / 60000));
    setFeatured(lane.ok && lane.data.featured && isStreamCard(lane.data.featured.stream) ? lane.data.featured : null);
    setError("");
    return data.rotates_at ? Date.parse(data.rotates_at) : null;
  }, []);

  // Reload on focus, then when MAGNet rotates (at least 30s apart), only while this tab is focused.
  // Between reloads the order never changes under the finger.
  useFocusEffect(useCallback(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      const next = await load();
      if (active) timer = setTimeout(() => { void tick(); }, Math.max(30000, (next ?? 0) + 1000 - Date.now()));
    };
    void tick();
    return () => { active = false; clearTimeout(timer); };
  }, [load]));

  const retry = () => { void load(); };
  const live = home?.live ?? [];
  const next = featured ? live[(live.findIndex(s => s.username === featured.stream.username) + 1) % live.length] : undefined;
  const upNext = next && next.username !== featured?.stream.username ? next : null;
  // One stream live and it is the featured one: Live now would only show it a second time (docs/DESIGN.md "Home").
  const repeated = live.length === 1 && featured?.stream.username === live[0].username;

  const header = (
    <View style={[styles.header, { paddingTop: insets.top, minHeight: 56 + insets.top }]}>
      <Text style={[type.logo, { color: colors.accentLight }]} accessibilityRole="header">S.V.E.R</Text>
      <View style={styles.mark}><MagnetMark /><Text style={type.label}>MAGNet</Text></View>
      <View style={styles.grow} />
      <IconButton icon={c => <SearchIcon color={c} />} label="Search" onPress={() => router.navigate("/browse?focus=1")} />
    </View>
  );

  return (
    <Page header={header} onRefresh={load} tabbed>
      {!account ? (
        <View style={styles.lead}>
          <Text style={[type.small, styles.grow]}>Find your people. Forge your legacy.</Text>
          <Button label="Log in" tone="quiet" compact onPress={() => router.push("/sign-in")} />
        </View>
      ) : null}
      {account?.deletion_due ? <Notice>This account is pending deletion.</Notice> : null}

      {!loaded ? (
        <>
          <CardSkeleton />
          <CardSkeleton />
          <CardSkeleton />
          <CardSkeleton />
        </>
      ) : !home ? (
        <Notice onRetry={retry}>{error}</Notice>
      ) : (
        <>
          {featured ? (
            <Frame style={styles.hero}>
              <Text style={type.label}>Featured now</Text>
              <StreamCard stream={featured.stream} size="full" minute={minute} viewerFaction={viewerFaction} />
              {featured.reason ? <Text style={[type.small, { color: colors.accentLight }]}>{featured.reason}</Text> : null}
              {featured.holding ? <Text style={type.small}>This stream isn’t available to you on MAGNet right now.</Text> : null}
              <Button label="Watch" onPress={() => router.push(`/watch/${featured.stream.username}`)} />
              {upNext ? <Text style={type.small}>Up next in rotation: {upNext.display_name}{upNext.category ? ` · ${upNext.category}` : ""}</Text> : null}
            </Frame>
          ) : null}
          {home.spotlights.length > 0 ? (
            <Rail>
              {home.spotlights.map(s => (
                <View key={s.stream.broadcast_id} style={styles.spot}>
                  <Tag label={s.reason} tone="accent" />
                  <StreamCard stream={s.stream} size="compact" minute={minute} viewerFaction={viewerFaction} />
                </View>
              ))}
            </Rail>
          ) : null}

          {account && home.following.length > 0 ? (
            <>
              <SectionHead title="Following" action={{ label: "View all", onPress: () => router.navigate("/following") }} />
              <Rail>
                {home.following.map(s => <StreamCard key={s.broadcast_id} stream={s} size="compact" minute={minute} viewerFaction={viewerFaction} />)}
              </Rail>
            </>
          ) : null}

          {error ? <Notice onRetry={retry}>{error}</Notice> : null}

          {live.length === 0 ? (
            <EmptyState title="Nothing live right now." action={{ label: "Watch Beacons", onPress: () => router.navigate("/beacons") }}>
              {home.recent.slice(0, 5).map(row => (
                <ChannelRow key={row.user.username} user={row.user} meta={`Live ${ago(row.ended_at)}`} onPress={() => router.push(`/watch/${row.user.username}`)} />
              ))}
            </EmptyState>
          ) : !repeated ? (
            <>
              <SectionHead title="Live now" note="Ordered by MAGNet, not by viewer count." />
              <View style={styles.trailList}>
                {live.map((stream, index) => {
                  const color = factionColor(stream.faction);
                  return (
                    <View key={stream.broadcast_id} style={styles.node}>
                      <View style={styles.rail}>
                        {index < live.length - 1 ? <View style={[styles.trail, { backgroundColor: color }]} /> : null}
                        <View style={[styles.diamond, { backgroundColor: color }]} />
                      </View>
                      <View style={styles.grow}>
                        <StreamCard stream={stream} size="full" minute={minute} viewerFaction={viewerFaction} />
                      </View>
                    </View>
                  );
                })}
              </View>
            </>
          ) : null}
        </>
      )}
    </Page>
  );
}

/** A horizontal row of compact cards that runs to the screen edges. */
function Rail({ children }: { children: ReactNode }) {
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} style={styles.railScroll} contentContainerStyle={styles.railContent}>
      {children}
    </ScrollView>
  );
}

function isSpot(value: unknown): value is Spot {
  return !!value && typeof value === "object" && typeof (value as Spot).reason === "string" && isStreamCard((value as Spot).stream);
}

function isRecent(value: unknown): value is Recent {
  if (!value || typeof value !== "object") return false;
  const row = value as { user?: { username?: unknown; display_name?: unknown }; ended_at?: unknown };
  return !!row.user && typeof row.user.username === "string" && typeof row.user.display_name === "string" && typeof row.ended_at === "string";
}

const styles = StyleSheet.create({
  header: { flexDirection: "row", alignItems: "center", gap: space.s, paddingHorizontal: space.l },
  mark: { flexDirection: "row", alignItems: "center", gap: space.s },
  grow: { flex: 1 },
  lead: { flexDirection: "row", alignItems: "center", gap: space.m },
  hero: { gap: space.m },
  spot: { gap: space.s, alignItems: "flex-start" },
  railScroll: { marginHorizontal: -space.l },
  railContent: { paddingHorizontal: space.l, gap: space.m },
  trailList: { gap: space.xl },
  node: { flexDirection: "row", gap: space.s },
  rail: { width: 20, alignItems: "center" },
  // The trail runs from this node's diamond down through the gap to the next one.
  trail: { position: "absolute", top: 12, bottom: -space.xl, width: 2, opacity: 0.45 },
  // docs/DESIGN.md's section-heading marker: an 8 px square turned 45°, no glow or rounding.
  diamond: { width: 8, height: 8, marginTop: 8, transform: [{ rotate: "45deg" }] },
});
