import type { Metadata } from "next";
import { redirect } from "next/navigation";

export const metadata: Metadata = { title: "Choose a faction | S.V.E.R", robots: { index: false, follow: false } };

// The faction step now lives in the onboarding wizard; this path stays for old links.
export default function ChooseSide() {
  redirect("/welcome");
}
