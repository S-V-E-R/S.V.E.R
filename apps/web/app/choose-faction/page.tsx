import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { isFaction } from "../../lib/factions";

export const metadata: Metadata = { title: "Choose a faction | S.V.E.R", robots: { index: false, follow: false } };

// The faction step lives in the onboarding wizard (/welcome); this path stays for old links.
export default async function ChooseFaction({ searchParams }: { searchParams: Promise<{ faction?: string }> }) {
  const { faction } = await searchParams;
  redirect(isFaction(faction) ? `/welcome?pick=${faction}` : "/welcome");
}
