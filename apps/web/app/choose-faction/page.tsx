import { redirect } from "next/navigation";
import { isFaction } from "../../lib/factions";

// The faction step lives in the onboarding wizard (/welcome); this path stays for old links.
export default async function ChooseFaction({ searchParams }: { searchParams: Promise<{ faction?: string }> }) {
  const { faction } = await searchParams;
  redirect(isFaction(faction) ? `/welcome?pick=${faction}` : "/welcome");
}
