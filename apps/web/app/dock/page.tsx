import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { Dock } from "./Dock";

export const metadata = { title: "S.V.E.R dock", robots: { index: false, follow: false } };

/** /dock: Creator Studio's essentials sized for an OBS Custom Browser Dock. Signing in comes back here. */
export default async function DockPage() {
  const account = await currentAccount();
  if (!account) redirect("/login?next=/dock");
  return <div className="obs-dock" data-theme={account.faction ?? "neutral"}><Dock username={account.username} /></div>;
}
