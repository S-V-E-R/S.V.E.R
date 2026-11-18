import { useState, type ReactNode } from "react";
import { Image, Pressable, RefreshControl, ScrollView, StyleSheet, Text, TextInput, View, type DimensionValue, type TextInputProps, type ViewStyle } from "react-native";
import { LinearGradient } from "expo-linear-gradient";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useAuth } from "./auth";
import { ChevronLeftIcon } from "./icons";
import { bg, fill, font, hexAlpha, ink, inkDim, inkMuted, line, live, onAccent, palette, scrim, space, surface, themeFor, tint } from "./theme";
import type { Avatar as AvatarSizes } from "./types";
import { absolute } from "./urls";

/**
 * Shared text styles (docs/DESIGN.md "Type"). Headings are `ink` here; pass the theme's
 * `colors.accentLight` alongside where a heading wears the accent.
 */
export const type = StyleSheet.create({
  logo: { fontFamily: font.display, fontSize: 28, color: ink },
  display: { fontFamily: font.display, fontSize: 22, color: ink },
  title: { fontFamily: font.display, fontSize: 18, color: ink },
  body: { fontFamily: font.body, fontSize: 15, lineHeight: 22, color: ink },
  strong: { fontFamily: font.bodySemi, fontSize: 15, lineHeight: 20, color: ink },
  small: { fontFamily: font.body, fontSize: 13, lineHeight: 18, color: inkMuted },
  label: { fontFamily: font.label, fontSize: 12, letterSpacing: 1.4, textTransform: "uppercase", color: inkDim },
});

export function Screen({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  const { colors } = useAuth();
  return (
    <LinearGradient colors={[colors.accentDeep, bg]} locations={[0, 0.7]} style={[styles.screen, style]}>
      {children}
    </LinearGradient>
  );
}

type Head = { eyebrow: string; title: string; lead?: string };

/** Eyebrow, Cinzel title and one lead line at the top of a page. */
export function PageHead({ eyebrow, title, lead }: Head) {
  const { colors } = useAuth();
  return (
    <View style={styles.head}>
      <Text style={type.label}>{eyebrow}</Text>
      <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">{title}</Text>
      {lead ? <Text style={type.small}>{lead}</Text> : null}
    </View>
  );
}

/**
 * A scrolling screen: the page tint, safe-area padding and pull to refresh. `header` (a HeaderBar)
 * sits above the scroll and owns the top inset. `tabbed` screens leave the bottom inset to the tab bar.
 */
export function Page({ head, header, onRefresh, tabbed, children }: { head?: Head; header?: ReactNode; onRefresh?: () => Promise<unknown>; tabbed?: boolean; children?: ReactNode }) {
  const insets = useSafeAreaInsets();
  const { colors } = useAuth();
  const [refreshing, setRefreshing] = useState(false);
  async function refresh() {
    if (!onRefresh) return;
    setRefreshing(true);
    await onRefresh();
    setRefreshing(false);
  }
  return (
    <Screen>
      {header}
      <ScrollView
        contentContainerStyle={[styles.page, { paddingTop: header ? space.l : insets.top + space.l, paddingBottom: space.xl + (tabbed ? 0 : insets.bottom) }]}
        keyboardShouldPersistTaps="handled"
        automaticallyAdjustKeyboardInsets
        refreshControl={onRefresh ? <RefreshControl refreshing={refreshing} onRefresh={() => { void refresh(); }} tintColor={colors.accent} colors={[colors.accent]} progressBackgroundColor={bg} /> : undefined}
      >
        {head ? <PageHead {...head} /> : null}
        {children}
      </ScrollView>
    </Screen>
  );
}

/** A stack screen's top bar: Back, a centered title and an optional control on the right. */
export function HeaderBar({ title, onBack, right }: { title?: string; onBack?: () => void; right?: ReactNode }) {
  const insets = useSafeAreaInsets();
  return (
    <View style={[styles.headerBar, { paddingTop: insets.top, minHeight: 56 + insets.top }]}>
      {onBack ? <IconButton icon={c => <ChevronLeftIcon color={c} />} label="Back" onPress={onBack} /> : <View style={styles.spacer} />}
      <Text style={[type.title, styles.headerTitle]} numberOfLines={1} accessibilityRole="header">{title}</Text>
      {right ?? <View style={styles.spacer} />}
    </View>
  );
}

/** A 44dp icon control. `label` is what a screen reader says. */
export function IconButton({ icon, label, onPress, tone = "plain", selected, disabled }: { icon: (color: string) => ReactNode; label: string; onPress: () => void; tone?: "plain" | "scrim"; selected?: boolean; disabled?: boolean }) {
  const { colors } = useAuth();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected, disabled }}
      disabled={disabled}
      hitSlop={4}
      onPress={onPress}
      style={({ pressed }) => [styles.iconButton, tone === "scrim" && { backgroundColor: scrim }, pressed && styles.pressed, disabled && styles.disabled]}
    >
      {icon(selected ? colors.accentLight : ink)}
    </Pressable>
  );
}

type Tone = "primary" | "quiet" | "danger";

/**
 * docs/DESIGN.md buttons. `primary` is beveled metal; `quiet` is the outline button (`selected` fills
 * it with the accent); `danger` is an outline in the live red, always behind a confirm. `kind` is the
 * older name for `tone`. `compact` sizes to its label instead of filling the row.
 */
export function Button({ label, onPress, disabled, tone, kind, compact, selected, icon, accessibilityLabel }: { label: string; onPress: () => void; disabled?: boolean; tone?: Tone; kind?: "primary" | "quiet"; compact?: boolean; selected?: boolean; icon?: (color: string) => ReactNode; accessibilityLabel?: string }) {
  const { colors } = useAuth();
  const t: Tone = tone ?? kind ?? "primary";
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      accessibilityState={{ disabled, selected }}
      disabled={disabled}
      onPress={onPress}
      style={[compact && styles.compact, disabled && styles.disabled]}
    >
      {({ pressed }) => {
        const filled = t === "primary" || (t === "quiet" && (selected || pressed));
        const color = filled ? onAccent : t === "danger" ? ink : colors.accentLight;
        const content = (
          <>
            {icon?.(color)}
            <Text style={[styles.buttonText, { color }]}>{label}</Text>
          </>
        );
        if (t === "primary") {
          return (
            <LinearGradient colors={[colors.accentLight, colors.accent, colors.accentDark]} locations={[0, 0.45, 1]} style={[styles.button, { borderColor: colors.accentLight }]}>
              {/* The bevel: a light top edge and a dark bottom edge, swapped for a dark top edge while pressed. */}
              <View pointerEvents="none" style={[styles.bevelTop, pressed && styles.bevelPressed]} />
              {!pressed ? <View pointerEvents="none" style={styles.bevelBottom} /> : null}
              {content}
            </LinearGradient>
          );
        }
        const border = t === "danger" ? live : colors.accent;
        return <View style={[styles.button, styles.quiet, { borderColor: border }, filled && { backgroundColor: colors.accent }, pressed && t === "danger" && { backgroundColor: hexAlpha(live, 0.2) }]}>{content}</View>;
      }}
    </Pressable>
  );
}

export function Field({ label, ...props }: { label: string } & TextInputProps) {
  const { colors } = useAuth();
  return (
    <View style={styles.field}>
      <Text style={styles.label}>{label}</Text>
      <TextInput
        {...props}
        accessibilityLabel={label}
        placeholderTextColor={inkDim}
        style={[styles.input, { borderColor: line(colors.accent) }]}
      />
    </View>
  );
}

/** An error or status line. `onRetry` adds a "Try again" button. */
export function Notice({ children, onRetry }: { children: string; onRetry?: () => void }) {
  return (
    <View style={styles.notice} accessibilityRole="alert">
      <Text style={[type.body, styles.grow]}>{children}</Text>
      {onRetry ? <Button label="Try again" tone="quiet" compact onPress={onRetry} /> : null}
    </View>
  );
}

type TagTone = "live" | "ally" | "accent" | "scrim" | "plain";

/** LIVE, ALLY, counts and labels. `scrim` (the default) sits over video and thumbnails; `plain` is an outline. */
export function Tag({ label, tone = "scrim" }: { label: string; tone?: TagTone }) {
  const { colors } = useAuth();
  const background = tone === "live" ? live : tone === "ally" || tone === "accent" ? colors.accent : tone === "scrim" ? scrim : "transparent";
  const color = tone === "ally" || tone === "accent" ? onAccent : tone === "plain" ? inkMuted : ink;
  return (
    <View style={[styles.tag, { backgroundColor: background }, tone === "plain" && { borderWidth: 1, borderColor: line(colors.accent) }]}>
      <Text style={[styles.tagText, { color }]}>{label}</Text>
    </View>
  );
}

function Box({ children, style, brackets }: { children: ReactNode; style?: ViewStyle; brackets: boolean }) {
  const { colors } = useAuth();
  return (
    <View style={[styles.frame, { borderColor: line(colors.accent), backgroundColor: surface }, style]}>
      {brackets ? <Brackets color={colors.accent} /> : null}
      {children}
    </View>
  );
}

/** The framed panel with corner brackets. Only for the featured hero, sign-in, empty states and the offline header. */
export function Frame({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  return <Box style={style} brackets>{children}</Box>;
}

/** A framed panel without brackets: stream tools, the pinned message, search results. */
export function Panel({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  return <Box style={style} brackets={false}>{children}</Box>;
}

function Brackets({ color }: { color: string }) {
  const arm = { backgroundColor: color };
  return (
    <View pointerEvents="none" style={StyleSheet.absoluteFill}>
      <View style={[styles.h, styles.tl, arm]} /><View style={[styles.v, styles.tl, arm]} />
      <View style={[styles.h, styles.tr, arm]} /><View style={[styles.v, styles.tr, arm]} />
      <View style={[styles.h, styles.bl, arm]} /><View style={[styles.v, styles.bl, arm]} />
      <View style={[styles.h, styles.br, arm]} /><View style={[styles.v, styles.br, arm]} />
    </View>
  );
}

/**
 * A square avatar framed in the user's faction color, or their initial when they have no picture.
 * `live` adds the red ring; the ring's space is kept either way so rows stay aligned.
 */
export function Avatar({ user, size, live: isLive }: { user: { display_name: string; avatar?: AvatarSizes; faction?: string | null }; size: 24 | 32 | 40 | 48 | 56; live?: boolean }) {
  const colors = palette[themeFor(user)];
  const uri = user.avatar?.[size <= 24 ? "64" : "160"];
  const box = { width: size, height: size, borderColor: line(colors.accent) };
  return (
    <View style={[styles.ring, isLive && { borderColor: live }]} accessible={false} importantForAccessibility="no-hide-descendants">
      {uri ? (
        <Image source={{ uri: absolute(uri) }} style={[styles.avatar, box]} accessibilityIgnoresInvertColors />
      ) : (
        <View style={[styles.avatar, styles.center, box, { backgroundColor: tint(colors.accent) }]}>
          <Text style={{ fontFamily: font.display, fontSize: Math.round(size * 0.45), color: colors.accentLight }}>{user.display_name.slice(0, 1).toUpperCase()}</Text>
        </View>
      )}
    </View>
  );
}

/** A static placeholder block while content loads. */
export function Skeleton({ w, h }: { w: DimensionValue; h: number }) {
  return <View style={[styles.skeleton, { width: w, height: h }]} />;
}

/** A stream card's shape while the list loads. */
export function CardSkeleton({ compact }: { compact?: boolean }) {
  return (
    <View style={[styles.gapS, compact && styles.compactCard]} accessible accessibilityLabel="Loading">
      <View style={[styles.skeleton, styles.thumb]} />
      <View style={styles.row}>
        <Skeleton w={32} h={32} />
        <View style={[styles.grow, styles.gapS]}>
          <Skeleton w="80%" h={14} />
          <Skeleton w="50%" h={12} />
        </View>
      </View>
    </View>
  );
}

/** A channel row's shape while the list loads. */
export function RowSkeleton() {
  return (
    <View style={[styles.row, styles.channelRow]} accessible accessibilityLabel="Loading">
      <Skeleton w={40} h={40} />
      <View style={[styles.grow, styles.gapS]}>
        <Skeleton w="60%" h={14} />
        <Skeleton w="35%" h={12} />
      </View>
    </View>
  );
}

/** "Nothing here" in a framed panel, with at most one action. */
export function EmptyState({ title, body, action, children }: { title: string; body?: string; action?: { label: string; onPress: () => void }; children?: ReactNode }) {
  const { colors } = useAuth();
  return (
    <Frame style={styles.empty}>
      <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">{title}</Text>
      {body ? <Text style={type.body}>{body}</Text> : null}
      {children}
      {action ? <Button label={action.label} onPress={action.onPress} /> : null}
    </Frame>
  );
}

/** docs/DESIGN.md "Section heading": diamond, Cinzel title, a rule to the edge and an optional "View all". */
export function SectionHead({ title, note, action }: { title: string; note?: string; action?: { label: string; onPress: () => void } }) {
  const { colors } = useAuth();
  return (
    <View style={styles.gapXs}>
      <View style={styles.row}>
        <View style={[styles.diamond, { backgroundColor: colors.accent }]} />
        <Text style={[type.display, { color: colors.accentLight }]} accessibilityRole="header">{title}</Text>
        <View style={[styles.rule, { backgroundColor: line(colors.accent) }]} />
        {action ? (
          <Pressable accessibilityRole="button" onPress={action.onPress} hitSlop={8} style={styles.sectionAction}>
            <Text style={[styles.actionText, { color: colors.accentLight }]}>{action.label}</Text>
          </Pressable>
        ) : null}
      </View>
      {note ? <Text style={[type.small, { color: inkDim }]}>{note}</Text> : null}
    </View>
  );
}

/** Three bars: Myria, Aetheron, Glint. The MAGNet mark from docs/DESIGN.md. */
export function MagnetMark() {
  return (
    <View style={styles.mark} accessibilityLabel="MAGNet">
      <View style={[styles.bar, { backgroundColor: palette.myria.accent }]} />
      <View style={[styles.bar, { backgroundColor: palette.aetheron.accent }]} />
      <View style={[styles.bar, { backgroundColor: palette.glint.accent }]} />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  page: { paddingHorizontal: space.l, gap: space.l },
  head: { gap: space.xs },
  headerBar: { flexDirection: "row", alignItems: "center", paddingHorizontal: space.xs },
  headerTitle: { flex: 1, textAlign: "center" },
  spacer: { width: 44 },
  iconButton: { width: 44, height: 44, alignItems: "center", justifyContent: "center" },
  frame: { borderWidth: 1, padding: space.l, position: "relative" },
  h: { position: "absolute", height: 2, width: 16 },
  v: { position: "absolute", width: 2, height: 16 },
  tl: { top: -1, left: -1 },
  tr: { top: -1, right: -1 },
  bl: { bottom: -1, left: -1 },
  br: { bottom: -1, right: -1 },
  button: { minHeight: 44, flexDirection: "row", gap: space.s, alignItems: "center", justifyContent: "center", paddingHorizontal: space.l, borderWidth: 1 },
  bevelTop: { position: "absolute", top: 0, left: 0, right: 0, height: 1, backgroundColor: "rgba(255,255,255,0.4)" },
  bevelPressed: { height: 2, backgroundColor: fill },
  bevelBottom: { position: "absolute", bottom: 0, left: 0, right: 0, height: 2, backgroundColor: fill },
  quiet: { backgroundColor: fill },
  compact: { alignSelf: "flex-start" },
  buttonText: { fontFamily: font.display, fontSize: 15, letterSpacing: 1.5 },
  pressed: { opacity: 0.85 },
  disabled: { opacity: 0.5 },
  field: { gap: space.s },
  label: { fontFamily: font.label, color: inkDim, letterSpacing: 1.4, textTransform: "uppercase", fontSize: 13 },
  input: { minHeight: 44, borderWidth: 1, color: ink, backgroundColor: fill, paddingHorizontal: space.m, fontFamily: font.body, fontSize: 16 },
  notice: { flexDirection: "row", alignItems: "center", gap: space.m, borderLeftWidth: 3, borderLeftColor: live, backgroundColor: surface, padding: space.m },
  grow: { flex: 1 },
  tag: { paddingHorizontal: space.s, paddingVertical: 2 },
  tagText: { fontFamily: font.label, fontSize: 12, letterSpacing: 1.44, textTransform: "uppercase" },
  ring: { borderWidth: 2, borderColor: "transparent", padding: 2 },
  avatar: { borderWidth: 1 },
  center: { alignItems: "center", justifyContent: "center" },
  skeleton: { backgroundColor: hexAlpha(ink, 0.06) },
  thumb: { width: "100%", aspectRatio: 16 / 9 },
  compactCard: { width: 240 },
  row: { flexDirection: "row", alignItems: "center", gap: space.s },
  channelRow: { minHeight: 64, paddingVertical: space.m },
  gapXs: { gap: space.xs },
  gapS: { gap: space.s },
  empty: { padding: space.xl, gap: space.m },
  diamond: { width: 8, height: 8, transform: [{ rotate: "45deg" }] },
  rule: { flex: 1, height: 1 },
  sectionAction: { minHeight: 44, justifyContent: "center" },
  actionText: { fontFamily: font.label, fontSize: 13, letterSpacing: 1.4, textTransform: "uppercase", textDecorationLine: "underline" },
  mark: { flexDirection: "row", alignItems: "flex-end", gap: 3, height: 14 },
  bar: { width: 4, height: 10 },
});

export { ink, inkMuted, inkDim, bg, hexAlpha };
