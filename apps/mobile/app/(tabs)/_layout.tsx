import { Image, Pressable, StyleSheet } from "react-native";
import { Tabs } from "expo-router";
import { isFaction } from "../../../web/lib/factions";
import { useAuth } from "../../src/auth";
import { BeaconsIcon, FactionIcon, FollowingIcon, HomeIcon } from "../../src/icons";
import { bg, inkMuted } from "../../src/theme";
import { hexAlpha } from "../../src/ui";
import { crestUrl } from "../../src/urls";

/**
 * docs/DESIGN.md "Phones": the main places in a bar along the bottom, one tap from any screen, as on
 * the website's BottomNav. Following joins when signed in. Watch, Log in and Alerts open over the tabs.
 */
export default function TabsLayout() {
  const { account, colors } = useAuth();
  const faction = account && isFaction(account.faction) ? account.faction : null;
  return (
    <Tabs
      screenOptions={{
        headerShown: false,
        sceneStyle: { backgroundColor: bg },
        tabBarStyle: { backgroundColor: bg, borderTopColor: hexAlpha(colors.accent, 0.32), borderTopWidth: 1 },
        tabBarActiveTintColor: colors.accentLight,
        tabBarInactiveTintColor: inkMuted,
        tabBarLabelStyle: styles.label,
        // The active place carries a 2 px accent bar along its top edge, like the website's bar.
        tabBarButton: ({ style, ...props }) => (
          <Pressable {...props} ref={undefined} style={[style, styles.button, props["aria-selected"] ? { borderTopColor: colors.accent } : null]} />
        ),
      }}
    >
      <Tabs.Screen name="index" options={{ title: "Home", tabBarIcon: ({ color }) => <HomeIcon color={color} /> }} />
      <Tabs.Screen name="following" options={{ title: "Following", href: account ? undefined : null, tabBarIcon: ({ color }) => <FollowingIcon color={color} /> }} />
      <Tabs.Screen name="beacons" options={{ title: "Beacons", tabBarIcon: ({ color }) => <BeaconsIcon color={color} /> }} />
      <Tabs.Screen
        name="account"
        options={{
          title: account ? "Account" : "Log in",
          tabBarIcon: ({ color }) => (faction ? <Image source={{ uri: crestUrl(faction) }} style={styles.crest} accessibilityIgnoresInvertColors /> : <FactionIcon color={color} />),
        }}
      />
    </Tabs>
  );
}

const styles = StyleSheet.create({
  label: { fontFamily: "BarlowCondensed_600SemiBold", fontSize: 11, letterSpacing: 0.66, textTransform: "uppercase" },
  button: { flex: 1, borderTopWidth: 2, borderTopColor: "transparent" },
  crest: { width: 20, height: 20 },
});
