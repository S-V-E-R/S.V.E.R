import { useCallback, useState } from "react";
import { Image, Pressable, RefreshControl, ScrollView, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../src/api";
import { useAuth } from "../src/auth";
import { factionColor, factionLabel } from "../src/brand";
import { isFaction } from "../../web/lib/factions";
import { useLoad } from "../src/load";
import { rotationOrder } from "../src/streams";
import { isStreamCard, type MagnetLane, type RecentChannel, type SearchResult, type StreamCard } from "../src/types";
import { Button, Field, Frame, MagnetMark, Notice, Screen, Tag, ink, inkDim, inkMuted } from "../src/ui";
import { absolute, crestUrl } from "../src/urls";

export default function MagnetScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [items, setItems] = useState<StreamCard[]>([]);
  const [recent, setRecent] = useState<RecentChannel[]>([]);
  const [featured, setFeatured] = useState<MagnetLane["featured"]>(null);
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<SearchResult | null>(null);
  const [searchError, setSearchError] = useState("");

  const load = useCallback(async () => {
    const [live, lane] = await Promise.all([
      send<{ items?: unknown; recent?: unknown }>("GET", "/api/discovery/live"),
      send<MagnetLane>("GET", "/api/magnet/global"),
    ]);
    if (!live.ok) {
      setError(live.error);
      return;
    }
    const cards = Array.isArray(live.data.items) ? live.data.items.filter(isStreamCard) : [];
    setItems(rotationOrder(cards));
    setRecent(Array.isArray(live.data.recent) ? live.data.recent.filter(isRecent) : []);
    setFeatured(lane.ok && lane.data.featured && isStreamCard(lane.data.featured.stream) ? lane.data.featured : null);
    setError("");
  }, []);

  useLoad(load, 30000);

  async function refresh() {
    setRefreshing(true);
    await load();
    setRefreshing(false);
  }

  async function search() {
    const q = query.trim();
    if (q.length < 2 || q.length > 50) {
      setSearchError("Search for 2–50 characters.");
      return;
    }
    const result = await send<SearchResult>("GET", `/api/search?q=${encodeURIComponent(q)}`);
    if (!result.ok) {
      setSearchError(result.error);
      return;
    }
    setSearchError("");
    setFound(result.data);
  }

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 12, paddingBottom: insets.bottom + 24, paddingHorizontal: 16, gap: 16 }}
        refreshControl={<RefreshControl refreshing={refreshing} onRefresh={() => { void refresh(); }} tintColor={colors.accent} />}
      >
        <View style={styles.top}>
          <View>
            <Text style={[styles.logo, { color: colors.accentLight }]}>S.V.E.R</Text>
            <View style={styles.markRow}><MagnetMark /><Text style={styles.kicker}>MAGNet</Text></View>
          </View>
          <Pressable accessibilityRole="button" onPress={() => router.push("/sign-in")} style={styles.account}>
            <Text style={[styles.accountText, { color: colors.accentLight }]}>{account ? account.username : "Log in"}</Text>
          </Pressable>
        </View>
        <Text style={styles.lead}>Every live stream gets a turn here, whether it has 3 viewers or 3,000.</Text>
        {account?.deletion_due ? <Notice>This account is pending deletion.</Notice> : null}
        {error ? <Notice>{error}</Notice> : null}
        <View style={styles.search}>
          <Field label="Search channels" value={query} onChangeText={setQuery} autoCapitalize="none" autoCorrect={false} onSubmitEditing={() => { void search(); }} />
          <Button label="Search" kind="quiet" onPress={() => { void search(); }} />
        </View>
        {searchError ? <Notice>{searchError}</Notice> : null}
        {found ? (
          <Frame>
            <View style={styles.rowBetween}>
              <Text style={styles.section}>Channels</Text>
              <Pressable accessibilityRole="button" onPress={() => setFound(null)}><Text style={{ color: colors.accentLight }}>Clear</Text></Pressable>
            </View>
            {found.channels.length === 0 ? <Text style={styles.dim}>No channels match.</Text> : found.channels.map(channel => channel.username ? (
              <Pressable key={channel.username} accessibilityRole="button" onPress={() => router.push(`/watch/${channel.username}`)} style={styles.searchRow}>
                <Text style={styles.name}>{channel.display_name}</Text>
                <Text style={styles.dim}>@{channel.username}</Text>
              </Pressable>
            ) : null)}
            {found.categories.length > 0 ? <Text style={styles.dim}>Categories: {found.categories.map(category => category.name).join(" · ")}</Text> : null}
          </Frame>
        ) : null}
        {featured?.stream ? (
          <Pressable accessibilityRole="button" accessibilityLabel={`Featured: ${featured.stream.title}`} onPress={() => router.push(`/watch/${featured.stream.username}`)}>
            <Frame>
              <Text style={styles.kicker}>Featured now</Text>
              <Text style={[styles.section, { color: colors.accentLight }]}>{featured.stream.display_name}</Text>
              <Text style={styles.name}>{featured.stream.title}</Text>
              {featured.reason ? <Text style={styles.dim}>{featured.reason}</Text> : null}
              {featured.holding ? <Text style={styles.dim}>This stream isn’t available to you on MAGNet right now.</Text> : null}
            </Frame>
          </Pressable>
        ) : null}
        {items.length === 0 && !error ? (
          <Frame>
            <Text style={styles.section}>Nothing live right now.</Text>
            {recent.length === 0 ? <Text style={styles.dim}>Recently live channels show up here when a broadcast has ended.</Text> : recent.map(row => row.user.username ? (
              <Pressable key={row.user.username} accessibilityRole="button" onPress={() => router.push(`/watch/${row.user.username}`)} style={styles.searchRow}>
                <Text style={styles.name}>{row.user.display_name}</Text>
                <Text style={styles.dim}>Recently live</Text>
              </Pressable>
            ) : null)}
          </Frame>
        ) : (
          <View>
            {items.map((stream, index) => (
              <Node key={stream.broadcast_id} stream={stream} last={index === items.length - 1} />
            ))}
          </View>
        )}
      </ScrollView>
    </Screen>
  );
}

function Node({ stream, last }: { stream: StreamCard; last: boolean }) {
  const color = factionColor(stream.faction);
  const side = factionLabel(stream.faction);
  const thumb = absolute(stream.thumbnail);
  return (
    <View style={styles.nodeRow}>
      <View style={styles.rail}>
        {!last ? <View style={[styles.trail, { backgroundColor: color }]} /> : null}
        <View style={[styles.halo, { backgroundColor: color }]} />
        <View style={[styles.core, { backgroundColor: color }]} />
      </View>
      <Pressable accessibilityRole="button" accessibilityLabel={`${stream.title}, ${stream.display_name}`} onPress={() => router.push(`/watch/${stream.username}`)} style={styles.nodeCard}>
        <Frame>
          <View style={styles.cardTop}>
            <Image source={{ uri: thumb }} style={styles.thumb} accessibilityIgnoresInvertColors />
            <View style={styles.cardBody}>
              {isFaction(stream.faction) ? <Image source={{ uri: crestUrl(stream.faction) }} style={styles.crest} accessibilityIgnoresInvertColors /> : null}
              <View style={styles.tags}>
                <Tag label="Live" tone="live" />
                {stream.mature ? <Tag label="Mature" /> : null}
                {side ? <Tag label={side} tone="accent" /> : null}
                {stream.label ? <Tag label={stream.label} /> : null}
              </View>
              <Text style={styles.name} numberOfLines={2}>{stream.title}</Text>
              <Text style={styles.dim}>{stream.display_name}{stream.category ? ` · ${stream.category}` : ""}</Text>
              <Text style={styles.count}>{stream.viewers.toLocaleString()} watching</Text>
            </View>
          </View>
        </Frame>
      </Pressable>
    </View>
  );
}

function isRecent(value: unknown): value is RecentChannel {
  if (!value || typeof value !== "object") return false;
  const row = value as { user?: { username?: unknown; display_name?: unknown } };
  return !!row.user && typeof row.user.display_name === "string";
}

const styles = StyleSheet.create({
  top: { flexDirection: "row", justifyContent: "space-between", alignItems: "flex-end", gap: 12 },
  logo: { fontFamily: "Cinzel_800ExtraBold", fontSize: 28 },
  markRow: { flexDirection: "row", alignItems: "center", gap: 8, marginTop: 4 },
  kicker: { fontFamily: "BarlowCondensed_700Bold", color: inkDim, letterSpacing: 1.6, textTransform: "uppercase", fontSize: 13 },
  account: { minHeight: 44, justifyContent: "center", paddingHorizontal: 4 },
  accountText: { fontFamily: "Cinzel_700Bold", fontSize: 15 },
  lead: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  search: { gap: 12 },
  section: { fontFamily: "Cinzel_800ExtraBold", color: ink, fontSize: 22, marginBottom: 8 },
  rowBetween: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" },
  searchRow: { minHeight: 44, justifyContent: "center" },
  name: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 16 },
  dim: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 14, marginTop: 2 },
  count: { color: inkDim, fontFamily: "BarlowCondensed_600SemiBold", fontSize: 13, letterSpacing: 0.6, marginTop: 4 },
  nodeRow: { flexDirection: "row", gap: 12 },
  rail: { width: 28, alignItems: "center" },
  trail: { position: "absolute", top: 16, bottom: -8, width: 2, opacity: 0.45 },
  halo: { width: 22, height: 22, borderRadius: 11, opacity: 0.35, marginTop: 16 },
  core: { position: "absolute", top: 22, width: 10, height: 10, borderRadius: 5 },
  nodeCard: { flex: 1, marginBottom: 12 },
  cardTop: { flexDirection: "row", gap: 12 },
  thumb: { width: 96, height: 54, backgroundColor: "#000" },
  cardBody: { flex: 1, gap: 4 },
  tags: { flexDirection: "row", flexWrap: "wrap", gap: 6 },
  crest: { width: 28, height: 28 },
});
