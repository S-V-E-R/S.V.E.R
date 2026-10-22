import { useCallback, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { router, useLocalSearchParams } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { Chat } from "../../src/chat";
import { Board, Crowd } from "../../src/crowd";
import { useLoad } from "../../src/load";
import { HlsPlayer } from "../../src/player";
import { PlaysControls } from "../../src/plays";
import { SafetySheet, type SafetyTarget } from "../../src/safety";
import type { Live } from "../../src/types";
import { Button, Frame, Notice, Screen, Tag, inkMuted } from "../../src/ui";
import { playbackKey } from "../../src/urls";

export default function WatchScreen() {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const params = useLocalSearchParams<{ username: string }>();
  const username = typeof params.username === "string" ? params.username : "";
  const [live, setLive] = useState<Live | null>(null);
  const [error, setError] = useState("");
  const [accepted, setAccepted] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);

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

  // Follow state comes from the channel page's viewer relation; null hides the button (owner, signed out, not loaded).
  const [following, setFollowing] = useState<boolean | null>(null);
  const [followBusy, setFollowBusy] = useState(false);
  const loadChannel = useCallback(async () => {
    if (!username || !account) { setFollowing(null); return; }
    const result = await send<{ viewer?: { is_owner?: boolean; following?: boolean } }>("GET", `/api/channels/${encodeURIComponent(username)}`);
    setFollowing(result.ok && !result.data.viewer?.is_owner ? !!result.data.viewer?.following : null);
  }, [username, account]);
  useLoad(loadChannel);
  async function toggleFollow() {
    if (following === null) return;
    setFollowBusy(true);
    const result = await send<{ following: boolean }>(following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    setFollowBusy(false);
    if (result.ok) setFollowing(result.data.following);
    else setError(result.error);
  }
  const followButton = following !== null ? <Button label={following ? "Unfollow" : "Follow"} kind={following ? "quiet" : "primary"} disabled={followBusy} onPress={() => { void toggleFollow(); }} /> : null;

  const hls = live?.live ? live.playback?.hls ?? null : null;
  const warn = live?.live && live.mature_warn && !accepted;

  return (
    <Screen>
      <ScrollView contentContainerStyle={{ paddingTop: insets.top + 8, paddingBottom: insets.bottom + 24, paddingHorizontal: 16, gap: 16 }}>
        <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={() => router.back()} style={styles.back}>
          <Text style={{ color: colors.accentLight, fontFamily: "Cinzel_700Bold" }}>Back</Text>
        </Pressable>
        {!username ? <Notice>This channel does not exist.</Notice> : null}
        {error ? <Notice>{error}</Notice> : null}
        {username && !live && !error ? <Text style={styles.body}>Loading the stream…</Text> : null}
        {live && !live.live ? (
          <Frame>
            <Text style={[styles.title, { color: colors.accentLight }]}>{username}</Text>
            {live.hosting ? (
              <>
                <Text style={styles.body}>Hosting {live.hosting.display_name}.</Text>
                <Button label={`Watch ${live.hosting.display_name}`} onPress={() => router.replace(`/watch/${live.hosting!.username}`)} />
              </>
            ) : <Text style={styles.body}>Nothing is live on this channel right now.</Text>}
            {followButton ? <View style={styles.actions}>{followButton}</View> : null}
          </Frame>
        ) : null}
        {live?.live ? (
          <>
            <View style={styles.tags}>
              <Tag label={live.state === "RECONNECTING" ? "Reconnecting" : "Live"} tone="live" />
              {live.mature ? <Tag label="Mature" /> : null}
              {live.category ? <Tag label={live.category} /> : null}
            </View>
            <Text style={[styles.title, { color: colors.accentLight }]}>{live.title}</Text>
            <Text style={styles.body}>{username} · {live.viewers.toLocaleString()} watching</Text>
            {followButton}
            {!live.is_owner ? <Button label="Report stream" kind="quiet" onPress={() => setReporting({ type: "live_stream", id: live.broadcast_id, label: "stream", username, displayName: username })} /> : null}
            {/* Blocking the streamer leaves the stream; the server stops serving it to this account. */}
            <SafetySheet target={reporting} onClose={() => setReporting(null)} onBlocked={() => { setReporting(null); router.back(); }} />
            {live.mature_blocked ? <Frame><Text style={styles.body}>This stream is labeled mature, so it isn’t available on your account.</Text></Frame> : null}
            {live.banned ? <Frame><Text style={styles.body}>You’re banned from this channel, so the stream isn’t available while you’re signed in.</Text></Frame> : null}
            {warn ? (
              <Frame>
                <Text style={styles.body}>{live.title} is labeled mature. It may include strong language, horror or other mature themes.</Text>
                <View style={styles.actions}>
                  <Button label="Watch" onPress={() => setAccepted(true)} />
                  <Button label="Go back" kind="quiet" onPress={() => router.back()} />
                </View>
              </Frame>
            ) : null}
            {!live.mature_blocked && !live.banned && !warn && hls ? (
              <HlsPlayer key={`${playbackKey(hls)}:${attempt}`} url={hls} username={username} broadcastId={live.broadcast_id} isOwner={live.is_owner} />
            ) : null}
            {!live.mature_blocked && !live.banned && !warn && !hls ? (
              <Frame>
                <Text style={styles.body}>This stream is only offered on the direct path right now. This app plays the HLS address, and this broadcast isn’t serving one.</Text>
                <Button label="Check again" kind="quiet" onPress={() => { setAttempt(n => n + 1); void load(); }} />
              </Frame>
            ) : null}
            {/* SVER Plays' game controls (Plays channels only), then CrowdSync: polls, rally, counters, Surge and the board. */}
            <PlaysControls username={username} />
            <Crowd username={username} />
            <Board username={username} />
            <Chat username={username} />
          </>
        ) : null}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  back: { minHeight: 44, justifyContent: "center", alignSelf: "flex-start" },
  title: { fontFamily: "Cinzel_800ExtraBold", fontSize: 24 },
  body: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  tags: { flexDirection: "row", flexWrap: "wrap", gap: 6 },
  actions: { gap: 12, marginTop: 12 },
});
