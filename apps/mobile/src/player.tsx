import { useEffect, useMemo, useRef, useState } from "react";
import { AccessibilityInfo, Animated, AppState, Pressable, StyleSheet, Text, View } from "react-native";
import { useEvent } from "expo";
import { useVideoPlayer, VideoView } from "expo-video";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "./api";
import { PlayerEffects, setPlayerDelay } from "./crowd";
import { ChatIcon, ContractIcon, ExpandIcon, MutedIcon, VolumeIcon } from "./icons";
import { browserId } from "./session";
import { scrim, space } from "./theme";
import { Button, IconButton, Tag, type } from "./ui";
import { playbackKey } from "./urls";

/** The stage chrome: Live/Reconnecting, mute, rotate, and (sideways) the chat panel toggle. */
type Chrome = { reconnecting: boolean; landscape: boolean; onRotate: () => void; chat?: { open: boolean; toggle: () => void } };
type Props = { url: string; username: string; broadcastId: string; isOwner: boolean } & Chrome;

/**
 * Plays the HLS address from GET /api/channels/{username}/live, filling its parent (the Watch stage).
 * WebRTC (WHEP) is what small streams prefer on the web; this build does not open it.
 * Heartbeats match the website: every 10 seconds, only while media time is advancing.
 */
export function HlsPlayer(props: Props) {
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  const [needsCheck, setNeedsCheck] = useState(false);
  if (failed) {
    return (
      <View style={styles.center}>
        <Text style={[type.body, styles.centerText]}>The stream couldn’t be played.</Text>
        <View style={styles.row}><Button label="Retry" onPress={() => { setFailed(false); setAttempt(n => n + 1); }} /></View>
      </View>
    );
  }
  return <Player {...props} generation={attempt} needsCheck={needsCheck} onFail={() => setFailed(true)} onTurnstile={() => setNeedsCheck(true)} />;
}

function Player({ url, generation, username, broadcastId, isOwner, needsCheck, onFail, onTurnstile, reconnecting, landscape, onRotate, chat }: Props & { generation: number; needsCheck: boolean; onFail: () => void; onTurnstile: () => void }) {
  // A new Bunny token is not a new stream. Retry (generation) and a new broadcast both take the latest URL.
  const identity = playbackKey(url);
  // `url` stays out of the dependency list so a token refresh does not reload the player.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const stableUrl = useMemo(() => url, [identity, generation]);
  const source = useMemo(() => ({ uri: stableUrl, contentType: "hls" as const }), [stableUrl]);
  const player = useVideoPlayer(source, instance => { instance.play(); });
  const { status } = useEvent(player, "statusChange", { status: player.status });
  const { muted } = useEvent(player, "mutedChange", { muted: player.muted });
  // "loading" before the first frame is the stage's spinner moment; after it, it's buffering.
  const [started, setStarted] = useState(false);
  if (status === "readyToPlay" && !started) setStarted(true);
  const base = useRef<number | null>(null);
  const last = useRef(-1);
  const onFailRef = useRef(onFail);
  const onTurnstileRef = useRef(onTurnstile);
  useEffect(() => {
    onFailRef.current = onFail;
    onTurnstileRef.current = onTurnstile;
  });

  useEffect(() => {
    if (status === "error") onFailRef.current();
  }, [status]);

  // Effects and poll windows follow this phone's picture: its distance from live, from the HLS
  // program date (EXT-X-PROGRAM-DATE-TIME). Without that tag, keep the 4 s default.
  useEffect(() => {
    const timer = setInterval(() => {
      const offset = player.currentOffsetFromLive;
      if (offset != null && Number.isFinite(offset) && offset >= 0) setPlayerDelay(Math.round(offset * 1000));
    }, 2000);
    return () => clearInterval(timer);
  }, [player]);

  useEffect(() => {
    if (isOwner) return;
    let id = "";
    void browserId().then(value => { id = value; });
    const timer = setInterval(() => {
      if (!id || AppState.currentState !== "active" || !player.playing) return;
      if (player.currentTime <= last.current) return;
      if (base.current === null) base.current = player.currentTime;
      last.current = player.currentTime;
      void send<{ recorded: boolean; needs_turnstile?: boolean }>("POST", `/api/channels/${encodeURIComponent(username)}/live/beat`, {
        broadcast_id: broadcastId,
        browser_id: id,
        visible: true,
        media_time: Math.max(0, player.currentTime - base.current),
      }).then(result => {
        if (result.ok && result.data.needs_turnstile) onTurnstileRef.current();
      });
    }, 10000);
    return () => clearInterval(timer);
  }, [broadcastId, isOwner, player, username]);

  // Tap the stage to show or hide the chrome; it hides 3.5 s after the last touch, and stays while a screen reader is on.
  const [shown, setShown] = useState(true);
  const [touches, setTouches] = useState(0);
  const [reader, setReader] = useState(false);
  const [calm, setCalm] = useState(false);
  const [fade] = useState(() => new Animated.Value(1));
  // Sideways, the bar sits above the home indicator so its first touch reaches the buttons.
  const insets = useSafeAreaInsets();
  const bottom = landscape ? insets.bottom : 0;
  const visible = shown || reader;
  const touch = () => { setShown(true); setTouches(n => n + 1); };
  useEffect(() => {
    void AccessibilityInfo.isScreenReaderEnabled().then(setReader);
    void AccessibilityInfo.isReduceMotionEnabled().then(setCalm);
    const a = AccessibilityInfo.addEventListener("screenReaderChanged", setReader);
    const b = AccessibilityInfo.addEventListener("reduceMotionChanged", setCalm);
    return () => { a.remove(); b.remove(); };
  }, []);
  useEffect(() => {
    Animated.timing(fade, { toValue: visible ? 1 : 0, duration: calm ? 0 : 150, useNativeDriver: true }).start();
    if (!visible) return;
    const timer = setTimeout(() => setShown(false), 3500);
    return () => clearTimeout(timer);
  }, [visible, calm, fade, touches]);

  return (
    <View style={styles.fill}>
      <VideoView player={player} style={StyleSheet.absoluteFill} nativeControls={false} contentFit="contain" allowsPictureInPicture={false} />
      <PlayerEffects username={username} />
      <Pressable style={StyleSheet.absoluteFill} onPress={() => (shown ? setShown(false) : touch())} accessibilityRole="button" accessibilityLabel={visible ? "Hide player controls" : "Show player controls"} />
      <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, styles.chrome]}>
        {/* Always shown, above the fading bar. */}
        {needsCheck ? <Text style={[type.small, styles.check]}>This device still needs the security check, so this view may not be counted. The website can finish that check.</Text> : null}
        <Animated.View pointerEvents={visible ? "box-none" : "none"} style={[styles.bar, { opacity: fade, paddingBottom: bottom, minHeight: 44 + bottom }]}>
          <Tag tone="live" label={reconnecting ? "Reconnecting" : "Live"} />
          <View style={styles.fill} />
          {chat ? <IconButton icon={c => <ChatIcon color={c} />} label={chat.open ? "Hide chat" : "Show chat"} selected={chat.open} onPress={() => { touch(); chat.toggle(); }} /> : null}
          {/* The player is a native object; `muted` is its setter (expo-video), and mutedChange re-renders. */}
          {/* eslint-disable-next-line react-hooks/immutability */}
          <IconButton icon={c => (muted ? <MutedIcon color={c} /> : <VolumeIcon color={c} />)} label={muted ? "Unmute" : "Mute"} onPress={() => { touch(); player.muted = !muted; }} />
          <IconButton icon={c => (landscape ? <ContractIcon color={c} /> : <ExpandIcon color={c} />)} label={landscape ? "Exit full screen" : "Full screen"} onPress={() => { touch(); onRotate(); }} />
        </Animated.View>
      </View>
      <View pointerEvents="none" style={styles.always}>
        {reconnecting && !visible ? <Tag tone="live" label="Reconnecting" /> : null}
        {started && status === "loading" ? <Tag label="Buffering" /> : null}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: space.m, padding: space.l },
  centerText: { textAlign: "center" },
  row: { flexDirection: "row", gap: space.m },
  chrome: { justifyContent: "flex-end" },
  check: { backgroundColor: scrim, paddingHorizontal: space.s, paddingVertical: space.xs },
  bar: { flexDirection: "row", alignItems: "center", minHeight: 44, paddingHorizontal: space.s, backgroundColor: scrim },
  always: { position: "absolute", top: space.s, right: space.s, flexDirection: "row", gap: space.xs },
});
