import { useCallback, useEffect, useState, type ReactNode } from "react";
import { ActivityIndicator, Alert, DeviceEventEmitter, Keyboard, Pressable, ScrollView, StyleSheet, Text, View, useWindowDimensions } from "react-native";
import { router, useFocusEffect, useLocalSearchParams } from "expo-router";
import * as ScreenOrientation from "expo-screen-orientation";
import { StatusBar } from "expo-status-bar";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { StreamCard } from "../../src/cards";
import { Chat } from "../../src/chat";
import { BOARD_EVENT, Board, Crowd } from "../../src/crowd";
import { uptime, watching } from "../../src/format";
import { CheckIcon, ChevronDownIcon, CloseIcon, MoreIcon } from "../../src/icons";
import { useLoad } from "../../src/load";
import { HlsPlayer } from "../../src/player";
import { PlaysControls } from "../../src/plays";
import { SafetySheet, type SafetyTarget } from "../../src/safety";
import { SubscribeButton } from "../../src/subscribe";
import { bg, black, font, inkMuted, line, palette, space, themeFor } from "../../src/theme";
import { isChannelHeader, isChannelRedirect, streamCards, type ChannelHeader, type Live, type StreamCard as Card } from "../../src/types";
import { Avatar, Button, IconButton, Notice, SectionHead, Skeleton, type } from "../../src/ui";
import { playbackKey } from "../../src/urls";

type Pane = "play" | "chat";

// Sideways, the chat panel opens by default only if the viewer had it open last time this session.
let panelWasOpen = false;

const close = () => (router.canGoBack() ? router.back() : router.replace("/"));

export default function WatchScreen() {
  const insets = useSafeAreaInsets();
  const { width, height } = useWindowDimensions();
  const landscape = width > height;
  const { account, colors } = useAuth();
  const params = useLocalSearchParams<{ username: string }>();
  const username = typeof params.username === "string" ? params.username : "";
  const [live, setLive] = useState<Live | null>(null);
  const [error, setError] = useState("");
  const [accepted, setAccepted] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);
  const [header, setHeader] = useState<ChannelHeader | null>(null);
  const [following, setFollowing] = useState(false);
  const [followBusy, setFollowBusy] = useState(false);
  const [suggested, setSuggested] = useState<{ items: Card[]; minute: number } | null>(null);
  const [crowd, setCrowd] = useState(false);
  const [picked, setPicked] = useState<Pane | null>(null);
  const [dots, setDots] = useState({ play: false, chat: false });
  const [panel, setPanel] = useState(panelWasOpen);
  const [keyboard, setKeyboard] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [headerTry, setHeaderTry] = useState(0);

  const load = useCallback(async () => {
    if (!username) return;
    const result = await send<Live>("GET", `/api/channels/${encodeURIComponent(username)}/live?transport=hls`);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setError("");
    setLive(result.data);
  }, [username]);
  useLoad(load, 15000);

  // Only Watch turns sideways. The root layout locks the rest of the app upright.
  useFocusEffect(useCallback(() => {
    void ScreenOrientation.unlockAsync();
    return () => { void ScreenOrientation.lockAsync(ScreenOrientation.OrientationLock.PORTRAIT_UP); };
  }, []));

  // The channel header is heavy: once per open (and again after signing in), never on the 15 s poll.
  // A failed fetch tries again in 15 s, since it decides the Play pane (Plays, board) and Follow.
  // The CrowdSync check decides whether a channel without Plays or a board still has a Play pane.
  useEffect(() => {
    if (!username) return;
    let gone = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const path = `/api/channels/${encodeURIComponent(username)}`;
    void send<unknown>("GET", path).then(result => {
      if (gone) return;
      if (!result.ok) { retry = setTimeout(() => setHeaderTry(n => n + 1), 15000); return; }
      if (isChannelRedirect(result.data)) router.replace(`/watch/${encodeURIComponent(result.data.redirect_to)}`);
      else if (isChannelHeader(result.data)) {
        setHeader(result.data);
        setFollowing(result.data.viewer.following);
      }
    });
    void send<{ poll?: unknown; prediction?: unknown; counters?: unknown[]; rally?: unknown; surge?: unknown }>("GET", `${path}/crowd`).then(r => {
      if (!gone && r.ok && (r.data.poll || r.data.prediction || r.data.counters?.length || r.data.rally || r.data.surge)) setCrowd(true);
    });
    return () => { gone = true; clearTimeout(retry); };
  }, [username, account?.id, headerTry]);

  const offline = live?.live === false;
  useEffect(() => {
    if (!offline || !username) return;
    let gone = false;
    void send<{ items?: unknown }>("GET", `/api/channels/${encodeURIComponent(username)}/suggestions`).then(r => {
      if (!gone && r.ok) setSuggested({ items: streamCards(r.data.items).slice(0, 5), minute: Math.floor(Date.now() / 60000) });
    });
    return () => { gone = true; };
  }, [offline, username]);

  useEffect(() => {
    const show = Keyboard.addListener("keyboardDidShow", () => setKeyboard(true));
    const hide = Keyboard.addListener("keyboardDidHide", () => setKeyboard(false));
    return () => { show.remove(); hide.remove(); };
  }, []);

  const channel = header?.channel;
  const hasPlay = !!(channel?.plays || channel?.board || crowd);
  // Play opens first when the channel has Plays or a board; once the viewer picks, it never flips on its own.
  const tab: Pane = hasPlay ? picked ?? (channel?.plays || channel?.board ? "play" : "chat") : "chat";
  const choose = (pane: Pane) => { setPicked(pane); setDots(d => ({ ...d, [pane]: false })); };
  const onUnread = useCallback(() => setDots(d => (d.chat ? d : { ...d, chat: true })), []);
  useEffect(() => {
    const sub = DeviceEventEmitter.addListener(BOARD_EVENT, (detail: { channel: string }) => {
      if (detail.channel !== username.toLowerCase()) return;
      setCrowd(true);
      if (tab !== "play") setDots(d => (d.play ? d : { ...d, play: true }));
    });
    return () => sub.remove();
  }, [username, tab]);

  const name = channel?.display_name ?? username;
  async function setFollow(on: boolean) {
    setFollowBusy(true);
    const result = await send<{ following: boolean }>(on ? "PUT" : "DELETE", `/api/follows/${encodeURIComponent(username)}`);
    setFollowBusy(false);
    if (result.ok) setFollowing(result.data.following);
    else setError(result.error);
  }
  function follow() {
    if (!account) router.push("/sign-in");
    else if (!following) void setFollow(true);
    else Alert.alert(`Unfollow ${name}?`, undefined, [{ text: "Cancel", style: "cancel" }, { text: "Unfollow", style: "destructive", onPress: () => { void setFollow(false); } }]);
  }
  // ponytail: a tap locks the chosen side until Watch closes, so turning the phone no longer flips it
  // after a tap. Re-unlocking once the device matches needs the motion sensor (not installed).
  function rotate() {
    void ScreenOrientation.lockAsync(landscape ? ScreenOrientation.OrientationLock.PORTRAIT_UP : ScreenOrientation.OrientationLock.LANDSCAPE);
  }
  function togglePanel() {
    panelWasOpen = !panel;
    setPanel(!panel);
  }

  const streamer = palette[themeFor(channel ?? null)];
  const isLive = !!live?.live;
  const hls = live?.live ? live.playback?.hls ?? null : null;
  const warn = live?.live && live.mature_warn && !accepted;
  const readOnly = live?.live ? !!(live.mature_blocked || live.banned) : false;
  // Below 700 dp with the keyboard up, the streamer bar keeps one line so the composer and some chat stay in view.
  const tight = keyboard && height < 700;
  const sidePanel = landscape && isLive && panel;

  let stage: ReactNode = null;
  if (!username) stage = <Centered><Text style={[type.body, styles.centerText]}>This channel does not exist.</Text></Centered>;
  else if (!live) stage = error ? null : <ActivityIndicator color={colors.accent} style={styles.fill} accessibilityLabel="Loading" />;
  else if (!live.live) {
    const hosting = live.hosting;
    stage = (
      <Centered>
        <Text style={[type.display, { color: streamer.accent }]}>Offline</Text>
        {hosting ? <Actions><Button label={`Watch ${hosting.display_name}`} onPress={() => router.replace(`/watch/${encodeURIComponent(hosting.username)}`)} /></Actions> : null}
      </Centered>
    );
  } else if (live.mature_blocked) stage = <Centered><Text style={[type.body, styles.centerText]}>This stream is labeled mature, so it isn’t available on your account.</Text></Centered>;
  else if (live.banned) stage = <Centered><Text style={[type.body, styles.centerText]}>You’re banned from this channel, so the stream isn’t available while you’re signed in.</Text></Centered>;
  else if (warn) {
    stage = (
      <Centered>
        <Text style={[type.body, styles.centerText]}>{live.title} is labeled mature. It may include strong language, horror or other mature themes.</Text>
        <Actions>
          <Button label="Watch" onPress={() => setAccepted(true)} />
          <Button label="Go back" tone="quiet" onPress={close} />
        </Actions>
      </Centered>
    );
  } else if (!hls) {
    stage = (
      <Centered>
        <Text style={[type.body, styles.centerText]}>This stream is only offered on the direct path right now. This app plays the HLS address, and this broadcast isn’t serving one.</Text>
        <Actions><Button label="Check again" tone="quiet" onPress={() => { setAttempt(n => n + 1); void load(); }} /></Actions>
      </Centered>
    );
  } else {
    stage = (
      <HlsPlayer key={`${playbackKey(hls)}:${attempt}`} url={hls} username={username} broadcastId={live.broadcast_id} isOwner={live.is_owner}
        reconnecting={live.state === "RECONNECTING"} landscape={landscape} onRotate={rotate} chat={landscape ? { open: panel, toggle: togglePanel } : undefined} />
    );
  }

  const title = live?.live ? live.title : name;
  const meta = live?.live ? [name, live.category, watching(live.viewers), uptime(live.started_at)].filter(Boolean).join(" · ")
    : channel ? `${channel.follower_count.toLocaleString()} ${channel.follower_count === 1 ? "follower" : "followers"}` : "";
  const bar = live ? (
    <View style={[styles.streamer, { borderBottomColor: line(streamer.accent) }]}>
      <Avatar user={channel ?? { display_name: name }} size={40} live={isLive} />
      <Pressable style={styles.fill} disabled={tight} onPress={() => setExpanded(x => !x)} accessibilityRole="button" accessibilityLabel={[title, meta].filter(Boolean).join(". ")}
        accessibilityHint={expanded ? "Shows less of the title" : "Shows the whole title"}>
        <Text style={type.strong} numberOfLines={tight ? 1 : expanded ? undefined : 2}>{title}</Text>
        {meta && !tight ? <Text style={styles.meta} numberOfLines={1}>{meta}</Text> : null}
      </Pressable>
      {header && !header.viewer.is_owner ? (
        <View style={styles.barButtons}>
          {/* Kept mounted while the keyboard is up, so its status isn't fetched again. */}
          <View style={tight && styles.hidden}><SubscribeButton username={username} /></View>
          <Button compact label={following ? "Following" : "Follow"} tone={following ? "quiet" : "primary"} icon={following ? c => <CheckIcon color={c} /> : undefined} disabled={followBusy} onPress={follow} />
        </View>
      ) : null}
      {live.live && !live.is_owner ? <IconButton icon={c => <MoreIcon color={c} />} label="Report or block" onPress={() => setReporting({ type: "live_stream", id: live.broadcast_id, label: "stream", username, displayName: name })} /> : null}
    </View>
  ) : error ? <Notice onRetry={() => { void load(); }}>{error}</Notice> : username ? (
    <View style={styles.streamer} accessible accessibilityLabel="Loading">
      <Skeleton w={40} h={40} />
      <View style={[styles.fill, styles.gap]}><Skeleton w="70%" h={14} /><Skeleton w="45%" h={12} /></View>
    </View>
  ) : null;

  const panes = username && !offline && !(error && !live);
  return (
    <View style={[styles.root, landscape && styles.row]}>
      <StatusBar style="light" hidden={landscape} />
      <View style={landscape ? [styles.fill, { paddingLeft: insets.left, paddingRight: sidePanel ? 0 : insets.right }] : { paddingTop: insets.top }}>
        <View style={[landscape ? styles.fill : styles.stage, { borderBottomColor: line(streamer.accent), backgroundColor: offline ? streamer.accentDeep : black }]}>
          {stage}
          <View style={styles.close}>
            <IconButton tone="scrim" icon={c => <ChevronDownIcon color={c} />} label="Close stream" onPress={close} />
          </View>
        </View>
      </View>
      <View style={landscape ? (sidePanel ? [styles.side, { width: Math.min(400, Math.max(280, width * 0.34)), paddingRight: insets.right, borderLeftColor: line(colors.accent) }] : styles.hidden) : styles.rest}>
        {landscape ? (
          <View style={styles.sideHead}>
            <Text style={[type.title, styles.fill]} accessibilityRole="header">Chat</Text>
            <IconButton icon={c => <CloseIcon color={c} />} label="Hide chat" onPress={togglePanel} />
          </View>
        ) : bar}
        {!landscape && error && live ? <Notice onRetry={() => { void load(); }}>{error}</Notice> : null}
        {!landscape && panes && hasPlay ? (
          <Tabs value={tab} onChange={choose} items={[{ key: "play", label: "Play", dot: dots.play && tab !== "play" }, { key: "chat", label: "Chat", dot: dots.chat && tab !== "chat" }]} />
        ) : null}
        {panes ? (
          <>
            <View style={[styles.fill, (landscape || tab !== "play") && styles.hidden]}>
              <ScrollView contentContainerStyle={[styles.play, { paddingBottom: insets.bottom + space.m }]} keyboardShouldPersistTaps="handled" automaticallyAdjustKeyboardInsets>
                {/* SVER Plays' game controls (Plays channels only), then CrowdSync: polls, rally, counters, Surge and the board. */}
                <PlaysControls username={username} />
                <Crowd username={username} />
                <Board username={username} />
                <Text style={type.small}>Polls, predictions, Rally and Surge show up here when the streamer starts them.</Text>
              </ScrollView>
            </View>
            <View style={[styles.fill, !landscape && tab !== "chat" && styles.hidden]}>
              <Chat username={username} active={landscape ? sidePanel : tab === "chat"} onUnread={onUnread} readOnly={readOnly} />
            </View>
          </>
        ) : null}
        {offline && !landscape ? (
          <ScrollView contentContainerStyle={[styles.offline, { paddingBottom: insets.bottom + space.xl }]}>
            {channel?.bio ? <Text style={type.small} numberOfLines={3}>{channel.bio}</Text> : null}
            {suggested?.items.length ? (
              <>
                <SectionHead title="Live now" />
                <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.rail}>
                  {suggested.items.map(stream => (
                    <StreamCard key={stream.broadcast_id} stream={stream} size="compact" minute={suggested.minute} viewerFaction={account?.faction ?? null}
                      onPress={() => router.replace(`/watch/${encodeURIComponent(stream.username)}`)} />
                  ))}
                </ScrollView>
              </>
            ) : null}
          </ScrollView>
        ) : null}
      </View>
      {/* Blocking the streamer leaves the stream; the server stops serving it to this account. */}
      <SafetySheet target={reporting} onClose={() => setReporting(null)} onBlocked={() => { setReporting(null); close(); }} />
    </View>
  );
}

function Centered({ children }: { children: ReactNode }) {
  return <View style={styles.center}>{children}</View>;
}

function Actions({ children }: { children: ReactNode }) {
  return <View style={styles.actions}>{children}</View>;
}

/** The Play / Chat switch. The dot marks a pane that changed while it was tucked away. */
function Tabs({ items, value, onChange }: { items: { key: Pane; label: string; dot: boolean }[]; value: Pane; onChange: (pane: Pane) => void }) {
  const { colors } = useAuth();
  return (
    <View style={[styles.tabs, { borderBottomColor: line(colors.accent) }]} accessibilityRole="tablist">
      {items.map(item => {
        const selected = item.key === value;
        return (
          <Pressable key={item.key} accessibilityRole="tab" accessibilityState={{ selected }} accessibilityLabel={item.dot ? `${item.label}, new activity` : item.label}
            onPress={() => onChange(item.key)} style={[styles.tab, selected && { borderBottomColor: colors.accent }]}>
            <Text style={[styles.tabText, { color: selected ? colors.accentLight : inkMuted }]}>{item.label}</Text>
            {item.dot ? <View style={[styles.dot, { backgroundColor: colors.accent }]} /> : null}
          </Pressable>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, backgroundColor: black },
  row: { flexDirection: "row" },
  fill: { flex: 1 },
  gap: { gap: space.s },
  hidden: { display: "none" },
  stage: { width: "100%", aspectRatio: 16 / 9, borderBottomWidth: 1 },
  close: { position: "absolute", top: space.xs, left: space.xs },
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: space.m, padding: space.l },
  centerText: { textAlign: "center" },
  actions: { flexDirection: "row", gap: space.m },
  rest: { flex: 1, backgroundColor: bg },
  side: { backgroundColor: bg, borderLeftWidth: 1 },
  sideHead: { flexDirection: "row", alignItems: "center", minHeight: 44, paddingLeft: space.m },
  barButtons: { flexDirection: "row", gap: space.s },
  streamer: { flexDirection: "row", alignItems: "center", gap: space.s, minHeight: 64, paddingVertical: space.s, paddingHorizontal: space.m, borderBottomWidth: 1 },
  meta: { fontFamily: font.labelSemi, fontSize: 12, letterSpacing: 1, textTransform: "uppercase", color: inkMuted, marginTop: 2 },
  tabs: { flexDirection: "row", borderBottomWidth: 1 },
  tab: { flex: 1, minHeight: 44, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: space.s, borderBottomWidth: 2, borderBottomColor: "transparent" },
  tabText: { fontFamily: font.label, fontSize: 13, letterSpacing: 1.4, textTransform: "uppercase" },
  dot: { width: 6, height: 6, transform: [{ rotate: "45deg" }] },
  play: { padding: space.m, gap: space.m },
  offline: { padding: space.l, gap: space.m },
  rail: { gap: space.m },
});
