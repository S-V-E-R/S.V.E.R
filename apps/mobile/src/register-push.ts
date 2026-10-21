import { useEffect } from "react";
import { router } from "expo-router";
import Constants from "expo-constants";
import * as Device from "expo-device";
import * as Linking from "expo-linking";
import * as Notifications from "expo-notifications";
import { AndroidImportance } from "expo-notifications";
import * as SecureStore from "expo-secure-store";
import { send } from "./api";
import { isExpoPushToken, notificationTarget } from "./push";
import { webOrigin } from "./urls";

const TOKEN = "sver.expo";
const OPENED = "sver.expo-opened";

Notifications.setNotificationHandler({
  handleNotification: async () => ({
    shouldShowBanner: true,
    shouldShowList: true,
    shouldPlaySound: true,
    shouldSetBadge: false,
  }),
});

export type PushState =
  | { state: "on" }
  | { state: "off" }
  | { state: "denied" }
  | { state: "simulator" }
  | { state: "unconfigured" }
  | { state: "failed"; message: string };

function projectId(): string | null {
  const fromEnv = process.env.EXPO_PUBLIC_EAS_PROJECT_ID?.trim();
  if (fromEnv) return fromEnv;
  const extra = Constants.expoConfig?.extra;
  if (extra && typeof extra === "object" && "eas" in extra) {
    const id = (extra as { eas?: { projectId?: unknown } }).eas?.projectId;
    if (typeof id === "string" && id.trim()) return id.trim();
  }
  const fromEas = Constants.easConfig?.projectId?.trim();
  return fromEas || null;
}

async function storedToken(): Promise<string | null> {
  try {
    return await SecureStore.getItemAsync(TOKEN);
  } catch {
    return null;
  }
}

/** Registers this phone while someone is signed in. A missing project id leaves the rest of the app usable. */
export async function registerDevice(): Promise<PushState> {
  const id = projectId();
  if (!id) return { state: "unconfigured" };
  if (!Device.isDevice) return { state: "simulator" };
  const current = await Notifications.getPermissionsAsync();
  let status = current.status;
  if (status !== "granted") status = (await Notifications.requestPermissionsAsync()).status;
  if (status !== "granted") return { state: "denied" };
  await Notifications.setNotificationChannelAsync("live", {
    name: "Go-live alerts",
    importance: AndroidImportance.HIGH,
  });
  await Notifications.setNotificationChannelAsync("alerts", {
    name: "Alerts",
    importance: AndroidImportance.DEFAULT,
  });
  try {
    const token = (await Notifications.getExpoPushTokenAsync({ projectId: id })).data;
    if (!isExpoPushToken(token)) return { state: "failed", message: "This phone returned a token the server will not store." };
    const saved = await send("POST", "/api/me/push/expo", { token });
    if (!saved.ok) return { state: "failed", message: saved.error };
    await SecureStore.setItemAsync(TOKEN, token);
    return { state: "on" };
  } catch (error) {
    return { state: "failed", message: error instanceof Error ? error.message : "This phone could not register for alerts." };
  }
}

/** Drops this phone's token. Called on log out, before the session cookie is cleared. */
export async function forgetDevice(): Promise<void> {
  const token = await storedToken();
  if (token && isExpoPushToken(token)) await send("DELETE", "/api/me/push/expo", { token });
  try {
    await SecureStore.deleteItemAsync(TOKEN);
  } catch {
    // The server row is already gone. A later sign-in registers a new token.
  }
}

export async function thisPhone(): Promise<boolean> {
  const token = await storedToken();
  return !!token && isExpoPushToken(token);
}

function openUrl(url: string) {
  const target = notificationTarget(url);
  if (!target) return;
  if (target.screen === "watch") router.push(`/watch/${encodeURIComponent(target.username)}`);
  else void Linking.openURL(`${webOrigin}${target.path}`);
}

async function openResponse(response: Notifications.NotificationResponse | null) {
  if (!response) return;
  const id = response.notification.request.identifier;
  const seen = await SecureStore.getItemAsync(OPENED).catch(() => null);
  if (seen === id) return;
  await SecureStore.setItemAsync(OPENED, id).catch(() => {});
  const data = response.notification.request.content.data;
  if (data && typeof data === "object" && "url" in data && typeof data.url === "string") openUrl(data.url);
}

/** Opens the channel when a go-live alert is tapped. Other alerts open the website. */
export function usePushOpen() {
  useEffect(() => {
    const sub = Notifications.addNotificationResponseReceivedListener(response => {
      void openResponse(response);
    });
    void Notifications.getLastNotificationResponseAsync().then(response => openResponse(response));
    return () => sub.remove();
  }, []);
}
