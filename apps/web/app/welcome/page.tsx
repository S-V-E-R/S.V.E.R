import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { Onboarding } from "./Onboarding";
import "../../styles/onboarding.css";

export const metadata: Metadata = { title: "Welcome | S.V.E.R", robots: { index: false, follow: false } };

/**
 * Onboarding after sign-up (docs/DESIGN.md, "Sign up"): choose your side, the welcome to your
 * faction, profile, follow creators, ready. Accounts without a faction are sent here too.
 */
export default async function WelcomePage({ searchParams }: { searchParams: Promise<{ pick?: string }> }) {
  const account = await currentAccount();
  if (!account) redirect("/signup");
  const { pick } = await searchParams;
  return <Onboarding username={account.username} current={account.faction} pick={pick ?? null} />;
}
