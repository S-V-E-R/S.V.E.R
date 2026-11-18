import type { ReactNode } from "react";
import { Image, Pressable, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { useAuth } from "./auth";
import { uptime, watching } from "./format";
import { font, hexAlpha, ink, inkMuted, line, space } from "./theme";
import type { Avatar as AvatarSizes, StreamCard as Card } from "./types";
import { Avatar, Tag, type } from "./ui";
import { absolute } from "./urls";

/**
 * docs/DESIGN.md "Stream card": a 16:9 still with tags in the corners, then avatar, title, name and
 * category. `minute` keys the still so the Android image cache fetches the current one.
 */
export function StreamCard({ stream, size, minute, viewerFaction, onPress }: { stream: Card; size: "full" | "compact"; minute: number; viewerFaction: string | null; onPress?: () => void }) {
  const { colors } = useAuth();
  const compact = size === "compact";
  const ally = viewerFaction != null && stream.faction === viewerFaction;
  const thumb = `${absolute(stream.thumbnail)}${stream.thumbnail.includes("?") ? "&" : "?"}v=${minute}`;
  const label = [stream.title, stream.display_name, stream.category, watching(stream.viewers), "live"].filter(Boolean).join(", ");
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      onPress={onPress ?? (() => router.push(`/watch/${stream.username}`))}
      style={({ pressed }) => [compact && styles.compact, pressed && styles.pressed]}
    >
      <View style={[styles.thumb, { backgroundColor: colors.accentDeep, borderColor: ally ? colors.accent : hexAlpha(ink, 0.08) }]}>
        {stream.category ? <Text style={styles.ghost} numberOfLines={1}>{stream.category}</Text> : null}
        <Image source={{ uri: thumb }} style={StyleSheet.absoluteFill} accessibilityIgnoresInvertColors />
        <View style={[styles.corner, styles.tags]}>
          <Tag label="Live" tone="live" />
          {ally ? <Tag label="Ally" tone="ally" /> : null}
          {stream.mature ? <Tag label="Mature" /> : null}
          {stream.label ? <Tag label={stream.label} /> : null}
        </View>
        <View style={[styles.corner, styles.bottomLeft]}><Tag label={watching(stream.viewers)} /></View>
        <View style={[styles.corner, styles.bottomRight]}>
          {stream.fresh ? <Tag label={`Started ${uptime(stream.started_at)} ago`} tone="accent" /> : <Tag label={uptime(stream.started_at)} />}
        </View>
      </View>
      <View style={styles.info}>
        <Avatar user={stream} size={32} />
        <View style={styles.text}>
          <Text style={[type.strong, compact && styles.compactTitle]} numberOfLines={2}>{stream.title}</Text>
          <Text style={type.small} numberOfLines={1}>{stream.display_name}</Text>
          {stream.category ? (
            <View style={[styles.chip, { borderColor: line(colors.accent) }]}>
              <Text style={styles.chipText} numberOfLines={1}>{stream.category}</Text>
            </View>
          ) : null}
        </View>
      </View>
    </Pressable>
  );
}

/** A channel in a list: avatar (red ring when live), name over `meta`, and a Live tag. `right` holds a row action. */
export function ChannelRow({ user, live, meta, onPress, right }: { user: { username: string | null; display_name: string; avatar?: AvatarSizes; faction?: string | null }; live?: boolean; meta?: string; onPress: () => void; right?: ReactNode }) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={[user.display_name, live ? "live" : null, meta].filter(Boolean).join(", ")}
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      <Avatar user={user} size={40} live={live} />
      <View style={styles.text}>
        <Text style={type.strong} numberOfLines={1}>{user.display_name}</Text>
        {meta ? <Text style={type.small} numberOfLines={1}>{meta}</Text> : null}
      </View>
      {live ? <Tag label="Live" tone="live" /> : null}
      {right}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  compact: { width: 240 },
  pressed: { opacity: 0.85 },
  thumb: { width: "100%", aspectRatio: 16 / 9, borderWidth: 1, alignItems: "center", justifyContent: "center", overflow: "hidden" },
  ghost: { fontFamily: font.display, fontSize: 18, color: hexAlpha(ink, 0.12), paddingHorizontal: space.m },
  corner: { position: "absolute" },
  tags: { top: space.s, left: space.s, right: space.s, flexDirection: "row", flexWrap: "wrap", gap: space.xs },
  bottomLeft: { bottom: space.s, left: space.s },
  bottomRight: { bottom: space.s, right: space.s },
  info: { flexDirection: "row", gap: space.s, marginTop: space.s },
  text: { flex: 1, gap: 2 },
  compactTitle: { fontSize: 14, lineHeight: 18 },
  chip: { alignSelf: "flex-start", borderWidth: 1, paddingHorizontal: space.xs, paddingVertical: 2, marginTop: 2 },
  chipText: { fontFamily: font.labelSemi, fontSize: 11, letterSpacing: 1, textTransform: "uppercase", color: inkMuted },
  row: { flexDirection: "row", alignItems: "center", gap: space.m, minHeight: 64, paddingVertical: space.m },
});
