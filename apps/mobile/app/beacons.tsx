import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppState, FlatList, Image, Pressable, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useVideoPlayer, VideoView } from "expo-video";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../src/api";
import { useAuth } from "../src/auth";
import { cookieHeader } from "../src/cookies";
import { browserId, loadJar } from "../src/session";
import { Tag, ink, inkMuted } from "../src/ui";
import { absolute } from "../src/urls";

type Item = {
  beacon: { id: string; title: string; category: string | null; likes: number };
  channel: { username: string | null; display_name: string; live?: boolean };
  liked: boolean;
  playback: { hd: string; sd: string } | null;
  thumbnail: string | null;
};
type FeedPage = { items: Item[]; has_more: boolean; next: number };

// The feed order comes from the server's fair rotation (docs/BEACONS.md); the seed keeps this
// session's order stable while paging. Never reordered here.
const SEED = Math.random().toString(36).slice(2, 12);

/**
 * The Beacons feed: one vertical video per screen, swipe to move. Videos start muted; a tap turns
 * sound on for the rest of the session. Only the Beacon on screen has a player.
 * ponytail: 18+ Beacons are left out (age_ack=false) until the app has the 18+ confirmation; comments,
 * share, report and mute stay on the website for now.
 */
export default function BeaconsScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [items, setItems] = useState<Item[]>([]);
  const [next, setNext] = useState(0);
  const [more, setMore] = useState(true);
  const [error, setError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [index, setIndex] = useState(0);
  const [height, setHeight] = useState(0);
  const [sound, setSound] = useState(false);
  const [cookie, setCookie] = useState<string | null>(null);
  const fetching = useRef(false);

  // Signed-in playback tickets belong to the account, so the video request carries the session cookie.
  useEffect(() => { void loadJar().then(jar => setCookie(cookieHeader(jar))); }, [account]);

  const loadMore = useCallback(async () => {
    if (fetching.current || !more) return;
    fetching.current = true;
    const result = await send<FeedPage>("GET", `/api/beacons/feed?seed=${SEED}&offset=${next}&age_ack=false`);
    fetching.current = false;
    setLoaded(true);
    if (!result.ok) { setError(result.error); return; }
    setError("");
    setMore(result.data.has_more);
    setNext(result.data.next);
    setItems(current => {
      const seen = new Set(current.map(i => i.beacon.id));
      return [...current, ...result.data.items.filter(i => !seen.has(i.beacon.id))];
    });
  }, [more, next]);
  // The first page on open; later pages come from the list's onEndReached, a few Beacons ahead.
  const latestLoad = useRef(loadMore);
  useEffect(() => { latestLoad.current = loadMore; });
  useEffect(() => { void latestLoad.current(); }, []);

  const onViewable = useCallback(({ viewableItems }: { viewableItems: { index?: number | null }[] }) => {
    const first = viewableItems[0];
    if (first?.index != null) setIndex(first.index);
  }, []);

  return (
    <View style={styles.screen} onLayout={e => setHeight(e.nativeEvent.layout.height)}>
      {height > 0 ? (
        <FlatList
          data={items}
          keyExtractor={item => item.beacon.id}
          pagingEnabled
          showsVerticalScrollIndicator={false}
          onViewableItemsChanged={onViewable}
          viewabilityConfig={{ itemVisiblePercentThreshold: 60 }}
          getItemLayout={(_, i) => ({ length: height, offset: height * i, index: i })}
          windowSize={3}
          onEndReached={() => { void loadMore(); }}
          onEndReachedThreshold={3}
          renderItem={({ item, index: i }) => (
            <BeaconCard item={item} height={height} active={i === index && cookie !== null} cookie={cookie ?? ""}
              sound={sound} onSound={() => setSound(true)} signedIn={!!account} accent={colors.accent} />
          )}
          ListEmptyComponent={
            <View style={[styles.empty, { height }]}>
              <Text style={styles.title}>{error || (loaded ? "No Beacons yet" : "Loading Beacons…")}</Text>
              {loaded && !error ? <Text style={styles.body}>Creators post short videos here that lead to their streams.</Text> : null}
            </View>
          }
        />
      ) : null}
      <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={() => router.back()} style={[styles.back, { top: insets.top + 8 }]}>
        <Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Back</Text>
      </Pressable>
    </View>
  );
}

function BeaconCard({ item, height, active, cookie, sound, onSound, signedIn, accent }: {
  item: Item; height: number; active: boolean; cookie: string; sound: boolean; onSound: () => void; signedIn: boolean; accent: string;
}) {
  const insets = useSafeAreaInsets();
  const { beacon: b, channel: c } = item;
  const [liked, setLiked] = useState(item.liked);
  const [likes, setLikes] = useState(b.likes);
  const [followed, setFollowed] = useState(false);
  const [message, setMessage] = useState("");
  const headers = cookie ? { Cookie: cookie } : undefined;

  async function like() {
    if (!signedIn) { router.push("/sign-in"); return; }
    const result = await send<{ liked: boolean; likes: number }>(liked ? "DELETE" : "PUT", `/api/beacons/${b.id}/like`);
    if (result.ok) { setLiked(result.data.liked); setLikes(result.data.likes); } else setMessage(result.error);
  }
  async function follow() {
    if (!signedIn) { router.push("/sign-in"); return; }
    if (!c.username) return;
    const result = await send("PUT", `/api/follows/${encodeURIComponent(c.username)}`);
    if (!result.ok) { setMessage(result.error); return; }
    setFollowed(true);
    // Credits this Beacon for the follow (docs/BEACONS.md "Counts").
    await send("POST", `/api/beacons/${b.id}/followed`);
  }
  async function watchLive() {
    if (!c.username) return;
    // Records the Beacon-to-stream handoff; the app opens its own Watch screen either way.
    await send("POST", `/api/beacons/${b.id}/live`, { browser_id: await browserId(), age_ack: false });
    router.push(`/watch/${c.username}`);
  }

  return (
    <View style={{ height, backgroundColor: "#000" }}>
      {active && item.playback ? (
        <BeaconVideo id={b.id} uri={absolute(item.playback.sd)} headers={headers} sound={sound} onSound={onSound} />
      ) : item.thumbnail ? (
        <Image source={{ uri: absolute(item.thumbnail), headers }} style={StyleSheet.absoluteFill} resizeMode="contain" accessibilityIgnoresInvertColors />
      ) : (
        <View style={styles.empty}><Text style={styles.body}>{item.playback ? "" : "This Beacon isn't available."}</Text></View>
      )}
      <View style={[styles.caption, { paddingBottom: insets.bottom + 16 }]} pointerEvents="box-none">
        <View style={{ flex: 1, gap: 4 }} pointerEvents="none">
          <Text style={styles.captionTitle} numberOfLines={3}>{b.title}</Text>
          <Text style={styles.body}>{c.username ? `@${c.username}` : c.display_name}{b.category ? ` · ${b.category}` : ""}</Text>
          {message ? <Text style={styles.body}>{message}</Text> : null}
        </View>
        <View style={styles.rail}>
          {c.live ? (
            <Pressable accessibilityRole="button" accessibilityLabel={`Watch ${c.display_name} live`} onPress={() => { void watchLive(); }} style={styles.railButton}>
              <Tag label="Live" tone="live" />
              <Text style={styles.railText}>Watch</Text>
            </Pressable>
          ) : null}
          <Pressable accessibilityRole="button" accessibilityLabel={liked ? "Unlike" : "Like"} accessibilityState={{ selected: liked }} onPress={() => { void like(); }} style={styles.railButton}>
            <View style={[styles.likeMark, { backgroundColor: liked ? accent : "transparent", borderColor: liked ? accent : ink }]} />
            <Text style={styles.railText}>{likes.toLocaleString()}</Text>
          </Pressable>
          {!followed && c.username ? (
            <Pressable accessibilityRole="button" accessibilityLabel={`Follow ${c.display_name}`} onPress={() => { void follow(); }} style={styles.railButton}>
              <Text style={[styles.railText, { color: accent }]}>Follow</Text>
            </Pressable>
          ) : null}
        </View>
      </View>
    </View>
  );
}

/** The on-screen Beacon. Sends the view heartbeat only while playback advances in the foreground. */
function BeaconVideo({ id, uri, headers, sound, onSound }: { id: string; uri: string; headers?: Record<string, string>; sound: boolean; onSound: () => void }) {
  const source = useMemo(() => ({ uri, headers }), [uri, headers?.Cookie]); // eslint-disable-line react-hooks/exhaustive-deps
  const player = useVideoPlayer(source, p => { p.loop = true; p.muted = true; p.play(); });
  // expo-video's player is a native object controlled by setting its properties.
  // eslint-disable-next-line react-hooks/immutability
  useEffect(() => { player.muted = !sound; }, [player, sound]);
  useEffect(() => {
    let last = -1;
    let browser = "";
    void browserId().then(value => { browser = value; });
    const timer = setInterval(() => {
      if (!browser || !player.playing || AppState.currentState !== "active" || player.currentTime === last) return;
      last = player.currentTime;
      void send("POST", `/api/beacons/${id}/beat`, { browser_id: browser, media_time: player.currentTime, visible: true, age_ack: false });
    }, 2000);
    return () => clearInterval(timer);
  }, [id, player]);
  function tap() {
    if (!sound) { onSound(); return; }
    if (player.playing) player.pause(); else player.play();
  }
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={sound ? "Pause or play" : "Turn sound on"} onPress={tap} style={StyleSheet.absoluteFill}>
      <VideoView player={player} style={StyleSheet.absoluteFill} contentFit="contain" nativeControls={false} allowsPictureInPicture={false} />
      {!sound ? <Text style={styles.soundHint}>Tap for sound</Text> : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: "#000" },
  back: { position: "absolute", left: 16, minHeight: 44, justifyContent: "center", paddingHorizontal: 4 },
  empty: { flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 24 },
  title: { color: ink, fontFamily: "Cinzel_800ExtraBold", fontSize: 22, textAlign: "center" },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 14, lineHeight: 20 },
  caption: { position: "absolute", left: 0, right: 0, bottom: 0, flexDirection: "row", alignItems: "flex-end", gap: 12, paddingHorizontal: 16, backgroundColor: "rgba(0,0,0,0.45)", paddingTop: 12 },
  captionTitle: { color: ink, fontFamily: "Barlow_600SemiBold", fontSize: 16, lineHeight: 22 },
  rail: { alignItems: "center", gap: 14 },
  railButton: { minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center", gap: 4 },
  railText: { color: ink, fontFamily: "BarlowCondensed_700Bold", fontSize: 13, letterSpacing: 1, textTransform: "uppercase" },
  // The like mark is the design system's diamond: outlined, filled with the accent once liked.
  likeMark: { width: 16, height: 16, borderWidth: 2, transform: [{ rotate: "45deg" }], marginBottom: 2 },
  soundHint: { position: "absolute", top: "45%", alignSelf: "center", color: ink, fontFamily: "BarlowCondensed_700Bold", letterSpacing: 1.2, textTransform: "uppercase", backgroundColor: "rgba(0,0,0,0.6)", paddingHorizontal: 10, paddingVertical: 6 },
});
