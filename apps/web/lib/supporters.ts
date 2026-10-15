import type { SupporterTier } from "../components/Supporter";

/** Platform supporters (docs/SUPPORTERS.md), as `GET /api/supporters` returns them. */
export type SupporterPlan = {
  tier: SupporterTier; pending_tier: SupporterTier | null; paid_through: string; auto_renew: boolean;
  card: boolean; months: number; since: string; next_cents: number;
};
export type SupporterMine = { plan: SupporterPlan | null; founder: { number: number; hidden: boolean } | null };
export type SupportersData = {
  available: boolean; sales_open: boolean; signed_in: boolean;
  tiers: { tier: SupporterTier; cents: number; terms: string }[];
  founders_claimed: number; founders_total: number;
  founders: { number: number; username: string | null; faction: string | null }[];
  mine: SupporterMine | null;
};

export const tierName: Record<SupporterTier, string> = { wolfsworn: "Wolfsworn", greycloak: "Greycloak" };
export const founderNumber = (n: number) => `#${String(n).padStart(3, "0")}`;
export const monthly = (cents: number) => `$${(cents / 100).toFixed(cents % 100 ? 2 : 0)} a month`;
