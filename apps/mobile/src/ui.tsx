import { type ReactNode } from "react";
import { Pressable, StyleSheet, Text, TextInput, View, type TextInputProps, type ViewStyle } from "react-native";
import { LinearGradient } from "expo-linear-gradient";
import { useAuth } from "./auth";
import { bg, ink, inkDim, inkMuted, live, palette, surface } from "./theme";

export function Screen({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  const { colors } = useAuth();
  return (
    <LinearGradient colors={[colors.accentDeep, bg]} locations={[0, 0.7]} style={[styles.screen, style]}>
      {children}
    </LinearGradient>
  );
}

export function Frame({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  const { colors } = useAuth();
  return (
    <View style={[styles.frame, { borderColor: hexAlpha(colors.accent, 0.32), backgroundColor: surface }, style]}>
      <Brackets color={colors.accent} />
      {children}
    </View>
  );
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

export function Button({ label, onPress, disabled, kind = "primary" }: { label: string; onPress: () => void; disabled?: boolean; kind?: "primary" | "quiet" }) {
  const { colors } = useAuth();
  const text = <Text style={[styles.buttonText, kind === "quiet" && { color: colors.accentLight }]}>{label}</Text>;
  return (
    <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={({ pressed }) => [pressed && styles.pressed, disabled && styles.disabled]}>
      {kind === "primary" ? (
        <LinearGradient colors={[colors.accentLight, colors.accent, colors.accentDark]} locations={[0, 0.45, 1]} style={styles.button}>
          {text}
        </LinearGradient>
      ) : (
        <View style={[styles.button, styles.quiet, { borderColor: colors.accent }]}>{text}</View>
      )}
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
        style={[styles.input, { borderColor: hexAlpha(colors.accent, 0.32) }]}
      />
    </View>
  );
}

export function Notice({ children }: { children: string }) {
  return (
    <View style={styles.notice} accessibilityRole="alert">
      <Text style={styles.noticeText}>{children}</Text>
    </View>
  );
}

export function Tag({ label, tone = "plain" }: { label: string; tone?: "live" | "plain" | "accent" }) {
  const { colors } = useAuth();
  const background = tone === "live" ? live : tone === "accent" ? colors.accent : "rgba(0,0,0,0.55)";
  const color = tone === "plain" ? ink : "#0A0A0C";
  return (
    <View style={[styles.tag, { backgroundColor: background }]}>
      <Text style={[styles.tagText, { color }]}>{label}</Text>
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

function hexAlpha(hex: string, alpha: number): string {
  const n = Math.round(alpha * 255).toString(16).padStart(2, "0");
  return `${hex}${n}`;
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  frame: { borderWidth: 1, padding: 16, position: "relative" },
  h: { position: "absolute", height: 2, width: 16 },
  v: { position: "absolute", width: 2, height: 16 },
  tl: { top: -1, left: -1 },
  tr: { top: -1, right: -1 },
  bl: { bottom: -1, left: -1 },
  br: { bottom: -1, right: -1 },
  button: { minHeight: 44, alignItems: "center", justifyContent: "center", paddingHorizontal: 16 },
  quiet: { backgroundColor: "rgba(0,0,0,0.35)", borderWidth: 1 },
  buttonText: { fontFamily: "Cinzel_700Bold", fontSize: 16, letterSpacing: 1.2, color: "#0A0A0C" },
  pressed: { opacity: 0.85 },
  disabled: { opacity: 0.5 },
  field: { gap: 8 },
  label: { fontFamily: "BarlowCondensed_700Bold", color: inkDim, letterSpacing: 1.4, textTransform: "uppercase", fontSize: 13 },
  input: { minHeight: 44, borderWidth: 1, color: ink, backgroundColor: "rgba(0,0,0,0.35)", paddingHorizontal: 12, fontFamily: "Barlow_400Regular", fontSize: 16 },
  notice: { borderLeftWidth: 3, borderLeftColor: live, backgroundColor: surface, padding: 12 },
  noticeText: { color: ink, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  tag: { paddingHorizontal: 6, paddingVertical: 2 },
  tagText: { fontFamily: "BarlowCondensed_700Bold", fontSize: 11, letterSpacing: 1, textTransform: "uppercase" },
  mark: { flexDirection: "row", alignItems: "flex-end", gap: 3, height: 14 },
  bar: { width: 4, height: 10 },
});

export { ink, inkMuted, inkDim, bg };
