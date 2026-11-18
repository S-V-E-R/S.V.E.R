import { Image, Pressable, StyleSheet } from "react-native";
import { Tabs } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { isFaction } from "../../../web/lib/factions";
import { useAuth } from "../../src/auth";
import { BeaconsIcon, BrowseIcon, FactionIcon, FollowingIcon, HomeIcon } from "../../src/icons";
import { bg, inkMuted, line } from "../../src/theme";
import { crestUrl } from "../../src/urls";

/**
 * docs/DESIGN.md "Phones": the main places in a bar along the bottom, one tap from any screen, as on
 * the website's BottomNav: Home, Browse, Beacons, then Following when signed in. 56dp plus the
 * bottom inset. Watch, Category, Log in and Alerts open over the tabs.
 */
export default function TabsLayout() {
  const { account, colors } = useAuth();
  const insets = useSafeAreaInsets();
  const faction = account && isFaction(account.faction) ? account.faction : null;
  return (
    <Tabs
      screenOptions={{
        headerShown: false,
        sceneStyle: { backgroundColor: bg },
        tabBarStyle: { backgroundColor: bg, borderTopColor: line(colors.accent), borderTopWidth: 1, height: 56 + insets.bottom },
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
      <Tabs.Screen name="browse" options={{ title: "Browse", tabBarIcon: ({ color }) => <BrowseIcon color={color} /> }} />
      <Tabs.Screen name="beacons" options={{ title: "Beacons", tabBarIcon: ({ color }) => <BeaconsIcon color={color} /> }} />
      <Tabs.Screen name="following" options={{ title: "Following", href: account ? undefined : null, tabBarIcon: ({ color }) => <FollowingIcon color={color} /> }} />
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
