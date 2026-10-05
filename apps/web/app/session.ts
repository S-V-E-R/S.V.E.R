import { cache } from "react";
import { cookies } from "next/headers";
import type { FactionSlug } from "../lib/factions";

export type Account = { username: string; faction: FactionSlug | null };

export const currentAccount = cache(async (): Promise<Account | null> => {
  const jar = await cookies();
  if (!jar.has("__Host-sver") && !jar.has("sver_dev")) return null;
  try {
    const response = await fetch(`${process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080"}/api/auth/me`, { headers: { cookie: jar.toString() }, cache: "no-store" });
    if (!response.ok) return null;
    const account = await response.json();
    if (typeof account.username !== "string") return null;
    const faction = ["myria", "aetheron", "glint"].includes(account.faction) ? account.faction as FactionSlug : null;
    return { username: account.username, faction };
  } catch {
    return null;
  }
});

/** Unread report notices or unacknowledged strikes, for the Settings dot (Module 2). */
export const hasAlerts = cache(async (): Promise<boolean> => {
  const jar = await cookies();
  try {
    const response = await fetch(`${process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080"}/api/me/alerts`, { headers: { cookie: jar.toString() }, cache: "no-store" });
    if (!response.ok) return false;
    const alerts = await response.json();
    return Number(alerts.unread_reports) > 0 || Number(alerts.new_strikes) > 0 || Number(alerts.notifications) > 0;
  } catch {
    return false;
  }
});
