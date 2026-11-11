import { useEffect, useMemo, useRef, useState } from "react";
import { AppState, StyleSheet, Text, View } from "react-native";
import { useVideoPlayer, VideoView } from "expo-video";
import { send } from "./api";
import { PlayerEffects, setPlayerDelay } from "./crowd";
import { browserId } from "./session";
import { Button, inkMuted } from "./ui";
import { playbackKey } from "./urls";

/**
 * Plays the HLS address from GET /api/channels/{username}/live.
 * WebRTC (WHEP) is what small streams prefer on the web; this build does not open it.
 * Heartbeats match the website: every 10 seconds, only while media time is advancing.
 */
export function HlsPlayer({ url, username, broadcastId, isOwner }: { url: string; username: string; broadcastId: string; isOwner: boolean }) {
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  const [needsCheck, setNeedsCheck] = useState(false);
  if (failed) {
    return (
      <View style={styles.stage}>
        <Text style={styles.note}>The stream couldn’t be played.</Text>
        <Button label="Retry" onPress={() => { setFailed(false); setAttempt(n => n + 1); }} />
      </View>
    );
  }
  return (
    <View>
      <Player url={url} generation={attempt} username={username} broadcastId={broadcastId} isOwner={isOwner} onFail={() => setFailed(true)} onTurnstile={() => setNeedsCheck(true)} />
      {needsCheck ? <Text style={styles.note}>This device still needs the security check, so this view may not be counted. The website can finish that check.</Text> : null}
    </View>
  );
}

function Player({ url, generation, username, broadcastId, isOwner, onFail, onTurnstile }: { url: string; generation: number; username: string; broadcastId: string; isOwner: boolean; onFail: () => void; onTurnstile: () => void }) {
  // A new Bunny token is not a new stream. Retry (generation) and a new broadcast both take the latest URL.
  const identity = playbackKey(url);
  // `url` stays out of the dependency list so a token refresh does not reload the player.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const stableUrl = useMemo(() => url, [identity, generation]);
  const source = useMemo(() => ({ uri: stableUrl, contentType: "hls" as const }), [stableUrl]);
  const player = useVideoPlayer(source, instance => { instance.play(); });
  const base = useRef<number | null>(null);
  const last = useRef(-1);
  const reported = useRef(false);
  const onFailRef = useRef(onFail);
  const onTurnstileRef = useRef(onTurnstile);
  useEffect(() => {
    onFailRef.current = onFail;
    onTurnstileRef.current = onTurnstile;
  });

  useEffect(() => {
    const timer = setInterval(() => {
      if (reported.current || player.status !== "error") return;
      reported.current = true;
      onFailRef.current();
    }, 2000);
    return () => clearInterval(timer);
  }, [player]);

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

  return (
    <View style={styles.stage}>
      <VideoView player={player} style={styles.video} nativeControls contentFit="contain" allowsPictureInPicture={false} />
      <PlayerEffects username={username} />
    </View>
  );
}

const styles = StyleSheet.create({
  stage: { width: "100%", aspectRatio: 16 / 9, backgroundColor: "#000", alignItems: "center", justifyContent: "center", gap: 12 },
  video: { width: "100%", height: "100%" },
  note: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22, marginTop: 8 },
});
