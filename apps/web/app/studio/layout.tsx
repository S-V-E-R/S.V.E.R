import Link from "next/link";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";
import { ReadinessBanner } from "../../components/Readiness";
import "../../styles/profiles.css";

export const metadata = { title: "Creator Studio | S.V.E.R", robots: { index: false, follow: false } };
const sections: [string, string][] = [["", "Overview"], ["header", "Page header"], ["song", "Song"], ["war-council", "War Council"], ["wall", "Wall"], ["schedule", "Schedule"], ["sponsors", "Sponsors"], ["setup", "Streaming setup"], ["blocks", "About blocks"], ["fan-art", "Fan Art"]];
export default async function StudioLayout({ children }: { children: React.ReactNode }) {
  const account = await currentAccount();
  if (!account) redirect("/login");
  return <div className="settings-page">
    <nav className="settings-nav" aria-label="Creator Studio"><span className="eyebrow">CREATOR STUDIO</span><Link href="/studio/stream">Live stream</Link><Link href="/studio/chat">Chat</Link><Link href="/studio/emotes">Emotes</Link>{sections.map(([href, label]) => <Link key={href} href={href ? `/studio/channel/${href}` : "/studio/channel"}>{label}</Link>)}<Link href={`/${account.username}`}>View channel</Link></nav>
    <div className="settings-body"><ReadinessBanner />{children}</div>
  </div>;
}
