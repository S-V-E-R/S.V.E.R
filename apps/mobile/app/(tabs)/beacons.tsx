import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ActivityIndicator, Alert, AppState, FlatList, Image, Pressable, StyleSheet, Text, View } from "react-native";
import { router, useIsFocused } from "expo-router";
import { useVideoPlayer, VideoView } from "expo-video";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { cookieHeader } from "../../src/cookies";
import { SafetySheet, type SafetyTarget } from "../../src/safety";
import { browserId, loadJar } from "../../src/session";
import { CheckIcon, FlagIcon, FollowingIcon, HeartIcon } from "../../src/icons";
import { black, font, scrim, space } from "../../src/theme";
import { EmptyState, IconButton, Tag, ink, type } from "../../src/ui";
import { absolute } from "../../src/urls";

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
 * Leaving the tab (or opening Watch over it) drops the player, so only one video plays at a time.
 * ponytail: 18+ Beacons are left out (age_ack=false) until the app has the 18+ confirmation; comments,
 * share and mute stay on the website for now.
 */
export default function BeaconsScreen() {
  const { account, colors } = useAuth();
  const focused = useIsFocused();
  const [items, setItems] = useState<Item[]>([]);
  const [next, setNext] = useState(0);
  const [more, setMore] = useState(true);
  const [error, setError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [index, setIndex] = useState(0);
  const [height, setHeight] = useState(0);
  const [sound, setSound] = useState(false);
  const [cookie, setCookie] = useState<string | null>(null);
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);
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
            <BeaconCard item={item} height={height} active={focused && i === index && cookie !== null} cookie={cookie ?? ""}
              sound={sound} onSound={() => setSound(true)} signedIn={!!account}
              onReport={() => setReporting({ type: "beacon", id: item.beacon.id, label: "Beacon", username: item.channel.username, displayName: item.channel.display_name })} />
          )}
          ListEmptyComponent={
            <View style={[styles.empty, { height }]}>
              {!loaded ? <ActivityIndicator color={colors.accent} size="large" accessibilityLabel="Loading" />
                : error ? <EmptyState title="Beacons didn't load" body={error} action={{ label: "Try again", onPress: () => { void loadMore(); } }} />
                : <EmptyState title="No Beacons yet" body="Creators post short videos here that lead to their streams." />}
            </View>
          }
        />
      ) : null}
      {/* A blocked creator's Beacons leave the feed now; the server leaves them out of later pages. */}
      <SafetySheet target={reporting} onClose={() => setReporting(null)}
        onBlocked={name => setItems(current => current.filter(i => i.channel.username !== name))} />
    </View>
  );
}

function BeaconCard({ item, height, active, cookie, sound, onSound, signedIn, onReport }: {
  item: Item; height: number; active: boolean; cookie: string; sound: boolean; onSound: () => void; signedIn: boolean; onReport: () => void;
}) {
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
  function follow() {
    if (!signedIn) { router.push("/sign-in"); return; }
    if (!followed) void setFollow();
    else Alert.alert(`Unfollow ${c.display_name}?`, undefined, [{ text: "Cancel", style: "cancel" }, { text: "Unfollow", style: "destructive", onPress: () => { void setFollow(); } }]);
  }
  async function setFollow() {
    if (!c.username) return;
    const result = await send<{ following: boolean }>(followed ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(c.username)}`);
    if (!result.ok) { setMessage(result.error); return; }
    setFollowed(result.data.following);
    if (!result.data.following) return;
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
    <View style={{ height, backgroundColor: black }}>
      {active && item.playback ? (
        <BeaconVideo id={b.id} uri={absolute(item.playback.sd)} headers={headers} sound={sound} onSound={onSound} />
      ) : item.thumbnail ? (
        <Image source={{ uri: absolute(item.thumbnail), headers }} style={StyleSheet.absoluteFill} resizeMode="contain" accessibilityIgnoresInvertColors />
      ) : (
        <View style={styles.empty}><Text style={[styles.body, { textAlign: "center" }]}>{item.playback ? "" : "This Beacon isn't available."}</Text></View>
      )}
      <View style={styles.caption} pointerEvents="box-none">
        <View style={{ flex: 1, gap: 4 }} pointerEvents="none">
          <Text style={styles.captionTitle} numberOfLines={3}>{b.title}</Text>
          <Text style={styles.body}>{c.username ? `@${c.username}` : c.display_name}{b.category ? ` · ${b.category}` : ""}</Text>
          {message ? <Text style={styles.body}>{message}</Text> : null}
        </View>
        <View style={styles.rail}>
          {c.live ? (
            <Pressable accessibilityRole="button" accessibilityLabel={`Watch ${c.display_name} live`} onPress={() => { void watchLive(); }} style={styles.live}>
              <Tag label="Live now" tone="live" />
            </Pressable>
          ) : null}
          <Rail caption={likes.toLocaleString()}>
            <IconButton icon={color => <HeartIcon color={color} filled={liked} />} label={`${liked ? "Unlike" : "Like"}, ${likes.toLocaleString()} likes`} selected={liked} onPress={() => { void like(); }} />
          </Rail>
          {c.username ? (
            <Rail caption={followed ? "Following" : "Follow"}>
              <IconButton icon={color => (followed ? <CheckIcon color={color} /> : <FollowingIcon color={color} />)}
                label={followed ? `Unfollow ${c.display_name}` : `Follow ${c.display_name}`} selected={followed} onPress={follow} />
            </Rail>
          ) : null}
          <Rail caption="Report">
            <IconButton icon={color => <FlagIcon color={color} />} label="Report or block" onPress={onReport} />
          </Rail>
        </View>
      </View>
    </View>
  );
}

/** A rail control with its caption under it; the control's own label is what a screen reader says. */
function Rail({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <View style={styles.railItem}>
      {children}
      <Text style={styles.railText} importantForAccessibility="no" accessibilityElementsHidden>{caption}</Text>
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
  screen: { flex: 1, backgroundColor: black },
  empty: { flex: 1, justifyContent: "center", padding: space.l },
  body: { ...type.small, color: ink },
  // The screen ends at the tab bar, so the caption's own padding is all it needs to clear it.
  caption: { position: "absolute", left: 0, right: 0, bottom: 0, flexDirection: "row", alignItems: "flex-end", gap: space.m, paddingHorizontal: space.l, paddingTop: space.m, paddingBottom: space.l, backgroundColor: scrim },
  captionTitle: { ...type.strong, lineHeight: 22 },
  rail: { alignItems: "center", gap: space.s },
  railItem: { alignItems: "center" },
  railText: { color: ink, fontFamily: font.label, fontSize: 11, letterSpacing: 1, textTransform: "uppercase" },
  live: { minHeight: 44, justifyContent: "center" },
  soundHint: { position: "absolute", top: "45%", alignSelf: "center", color: ink, fontFamily: font.label, letterSpacing: 1.2, textTransform: "uppercase", backgroundColor: scrim, paddingHorizontal: space.m, paddingVertical: space.s },
});
