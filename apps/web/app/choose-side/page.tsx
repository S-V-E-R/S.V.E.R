import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { ChooseSide } from "./ChooseSide";

export const metadata: Metadata = { title: "Choose your side | S.V.E.R" };

// Sign-up step 2 (docs/DESIGN.md, "Sign up"). Also where an account without a faction is sent to pick one.
export default async function ChooseSidePage() {
  const account = await currentAccount();
  if (!account) redirect("/signup");
  return <ChooseSide current={account.faction} />;
}
