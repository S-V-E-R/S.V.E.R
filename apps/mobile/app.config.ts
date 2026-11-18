import { AndroidConfig, withAndroidManifest } from "expo/config-plugins";
import type { ExpoConfig } from "expo/config";

const api = process.env.EXPO_PUBLIC_API_URL ?? "https://sver.tv";
const cleartext = api.startsWith("http://");
// CI sets the run number so each test APK installs over the last one.
const build = Number(process.env.SVER_BUILD_NUMBER) || 1;

const config: ExpoConfig = {
  name: "S.V.E.R",
  slug: "sver",
  scheme: "sver",
  version: "0.1.0",
  // The root layout locks portrait; Watch unlocks it for landscape (expo-screen-orientation).
  orientation: "default",
  userInterfaceStyle: "dark",
  icon: "./assets/icon.png",
  backgroundColor: "#07080B",
  ios: {
    supportsTablet: true,
    // iPad ignores orientation locks unless the app runs full screen.
    requireFullScreen: true,
    bundleIdentifier: "tv.sver.app",
    buildNumber: String(build),
    infoPlist: cleartext
      ? { NSAppTransportSecurity: { NSAllowsArbitraryLoads: true } }
      : { NSAppTransportSecurity: { NSAllowsArbitraryLoads: false } },
  },
  android: {
    package: "tv.sver.app",
    versionCode: build,
    adaptiveIcon: { backgroundColor: "#07080B", foregroundImage: "./assets/icon.png" },
  },
  plugins: [
    "expo-router",
    "expo-video",
    "expo-secure-store",
    "expo-font",
    ["expo-screen-orientation", { initialOrientation: "PORTRAIT_UP" }],
    [
      "expo-notifications",
      { color: "#F2F0EA", defaultChannel: "live" },
    ],
    [
      "expo-splash-screen",
      { backgroundColor: "#07080B", image: "./assets/splash.png", imageWidth: 200 },
    ],
  ],
  experiments: { typedRoutes: false },
  extra: {
    eas: process.env.EXPO_PUBLIC_EAS_PROJECT_ID
      ? { projectId: process.env.EXPO_PUBLIC_EAS_PROJECT_ID }
      : {},
  },
};

export default cleartext
  ? withAndroidManifest(config, mod => {
      const app = AndroidConfig.Manifest.getMainApplicationOrThrow(mod.modResults);
      app.$["android:usesCleartextTraffic"] = "true";
      return mod;
    })
  : config;
