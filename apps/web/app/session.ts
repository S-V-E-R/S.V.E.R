import { cache } from "react";
import { isFaction, type Faction } from "../lib/factions";
import { cookies } from "next/headers";

export const currentAccount = cache(async (): Promise<{ username: string; faction: Faction | null; email_verified: boolean; deletion_due: string | null } | null> => {
  const jar = await cookies();
  if (!jar.has("__Host-sver") && !jar.has("sver_dev")) return null;
  try {
    const response = await fetch(`${process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080"}/api/auth/me`, { headers: { cookie: jar.toString() }, cache: "no-store" });
    if (!response.ok) return null;
    const account = await response.json();
    return typeof account.username === "string" ? { username: account.username, faction: isFaction(account.faction) ? account.faction : null, email_verified: account.email_verified === true, deletion_due: account.deletion_due ?? null } : null;
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
