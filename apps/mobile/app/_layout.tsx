import "react-native-gesture-handler";
import { useEffect } from "react";
import { ActivityIndicator, Text, View } from "react-native";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import * as SplashScreen from "expo-splash-screen";
import { useFonts } from "expo-font";
import { Cinzel_700Bold } from "@expo-google-fonts/cinzel/700Bold";
import { Cinzel_800ExtraBold } from "@expo-google-fonts/cinzel/800ExtraBold";
import { Barlow_400Regular } from "@expo-google-fonts/barlow/400Regular";
import { Barlow_600SemiBold } from "@expo-google-fonts/barlow/600SemiBold";
import { BarlowCondensed_600SemiBold } from "@expo-google-fonts/barlow-condensed/600SemiBold";
import { BarlowCondensed_700Bold } from "@expo-google-fonts/barlow-condensed/700Bold";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { AuthProvider, useAuth } from "../src/auth";
import { bg, ink } from "../src/theme";

SplashScreen.preventAutoHideAsync().catch(() => {});

export default function Root() {
  const [loaded, error] = useFonts({
    Cinzel_700Bold, Cinzel_800ExtraBold,
    Barlow_400Regular, Barlow_600SemiBold,
    BarlowCondensed_600SemiBold, BarlowCondensed_700Bold,
  });
  if (!loaded && !error) return <View style={{ flex: 1, backgroundColor: bg }} />;
  return (
    <SafeAreaProvider>
      <AuthProvider>
        <Shell fontsFailed={!!error} />
      </AuthProvider>
    </SafeAreaProvider>
  );
}

function Shell({ fontsFailed }: { fontsFailed: boolean }) {
  const { ready } = useAuth();
  useEffect(() => {
    if (ready) void SplashScreen.hideAsync();
  }, [ready]);
  if (!ready) {
    return (
      <View style={{ flex: 1, backgroundColor: bg, alignItems: "center", justifyContent: "center" }}>
        <ActivityIndicator color={ink} />
      </View>
    );
  }
  return (
    <>
      <StatusBar style="light" />
      {fontsFailed ? <Text style={{ color: ink, backgroundColor: bg, padding: 8 }}>Typefaces didn’t load. Text is using the device fallback.</Text> : null}
      <Stack screenOptions={{ headerShown: false, contentStyle: { backgroundColor: bg } }} />
    </>
  );
}
