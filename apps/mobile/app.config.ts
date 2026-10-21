import { AndroidConfig, withAndroidManifest } from "@expo/config-plugins";
import type { ExpoConfig } from "expo/config";

const api = process.env.EXPO_PUBLIC_API_URL ?? "https://sver.tv";
const cleartext = api.startsWith("http://");

const config: ExpoConfig = {
  name: "S.V.E.R",
  slug: "sver",
  scheme: "sver",
  version: "0.1.0",
  orientation: "portrait",
  userInterfaceStyle: "dark",
  icon: "./assets/icon.png",
  backgroundColor: "#07080B",
  ios: {
    supportsTablet: true,
    bundleIdentifier: "tv.sver.app",
    infoPlist: cleartext
      ? { NSAppTransportSecurity: { NSAllowsArbitraryLoads: true } }
      : { NSAppTransportSecurity: { NSAllowsArbitraryLoads: false } },
  },
  android: {
    package: "tv.sver.app",
    adaptiveIcon: { backgroundColor: "#07080B", foregroundImage: "./assets/icon.png" },
  },
  plugins: [
    "expo-router",
    "expo-video",
    "expo-secure-store",
    "expo-font",
    [
      "expo-splash-screen",
      { backgroundColor: "#07080B", image: "./assets/splash.png", imageWidth: 200 },
    ],
  ],
  experiments: { typedRoutes: false },
};

export default cleartext
  ? withAndroidManifest(config, mod => {
      const app = AndroidConfig.Manifest.getMainApplicationOrThrow(mod.modResults);
      app.$["android:usesCleartextTraffic"] = "true";
      return mod;
    })
  : config;
