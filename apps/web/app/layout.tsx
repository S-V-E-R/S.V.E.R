import type { Metadata } from "next";
import { Barlow, Barlow_Condensed, Cinzel } from "next/font/google";
import Link from "next/link";
import SiteShell from "../components/SiteShell";
import { StaffRemovalAlerts } from "../components/StaffRemovalAlerts";
import { BellIcon, MagnetMark } from "../components/shell/Icons";
import { SideNav } from "../components/shell/SideNav";
import type { PeoplePage } from "../components/People";
import { apiGet } from "../lib/server-api";
import { themeFor } from "../lib/theme";
import { factionOf } from "../lib/factions";
import { Crest } from "../components/Crest";
import { currentAccount, hasAlerts } from "./session";
import "./globals.css";
import "../styles/profiles.css";
import "../styles/design.css";
import "../styles/site-pages.css";
// Module 4 pages (faction hubs, war map) and the shared stream shelves.
import "../styles/discovery.css";
import "../styles/factions.css";

// Type per docs/DESIGN.md "Type": self-hosted and subset by next/font.
const cinzel = Cinzel({ subsets: ["latin"], weight: ["700", "800"], variable: "--font-cinzel", display: "swap" });
const barlow = Barlow({ subsets: ["latin"], weight: ["400", "500", "600"], variable: "--font-barlow", display: "swap" });
const barlowCondensed = Barlow_Condensed({ subsets: ["latin"], weight: ["600", "700"], variable: "--font-barlow-condensed", display: "swap" });

const SITE_DESCRIPTION = "A live streaming platform built on community. Choose your faction, grow with your people, and get discovered: every live stream gets a fair turn.";

export const metadata: Metadata = {
  title: "S.V.E.R — Find Your People. Forge Your Legacy.",
  description: SITE_DESCRIPTION,
  applicationName: "S.V.E.R",
  openGraph: { siteName: "S.V.E.R", title: "S.V.E.R — Find Your People. Forge Your Legacy.", description: SITE_DESCRIPTION },
  twitter: { title: "S.V.E.R — Find Your People. Forge Your Legacy.", description: SITE_DESCRIPTION },
  robots: { index: false, follow: false },
};

/** Channels the viewer follows that are live now, from the first page of their follows. */
async function followingLive() {
  const page = (await apiGet<PeoplePage>("/api/me/following?live=true")).data;
  return (page?.items ?? []).map(item => item.user).filter(user => user.live && user.username);
}

export default async function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  const account = await currentAccount();
  const [alerts, live] = account ? await Promise.all([hasAlerts(), followingLive()]) : [false, []];
  const initial = account?.username.slice(0, 1).toUpperCase();

  const faction = factionOf(account?.faction);

  const actions = account
    ? <>
      <StaffRemovalAlerts />
      <Link href="/notifications" className="icon-button" aria-label={alerts ? "Notifications, new notices" : "Notifications"}><BellIcon />{alerts && <span className="alert-badge" aria-hidden="true" />}</Link>
      <Link href={`/${account.username}`} className="player-chip">
        <Crest faction={account.faction} initial={initial ?? "?"} size={36} label={faction?.name} />
        <span className="player-chip-text"><span className="player-chip-name">{account.username}</span>{faction && <span className="player-chip-title">{faction.title}</span>}</span>
      </Link>
    </>
    : <>
      <Link href="/login" className="topbar-link">Log in</Link>
      <Link href="/signup" className="button">Enlist</Link>
    </>;

  const sidebar = <>
    {account && (faction
      ? <Link href={`/${account.username}`} className="player-card frame">
        <Crest faction={account.faction} initial={initial ?? "?"} size={56} label={faction.name} />
        <span className="player-card-text"><span className="player-card-name">{account.username}</span><span className="player-card-faction">{faction.title}</span></span>
      </Link>
      : <Link href="/welcome" className="player-card frame unchosen">
        <Crest faction={null} initial={initial ?? "?"} size={56} />
        <span className="player-card-text"><span className="player-card-name">{account.username}</span><span className="player-card-faction">Choose your side →</span></span>
      </Link>)}
    <SideNav signedIn={!!account} faction={faction ? { name: faction.name, slug: faction.slug } : null} />
    {account && <section className="side-section" aria-labelledby="following-live">
      <div className="side-label"><span id="following-live">Following · live</span><span className="magnet"><MagnetMark />MAGNet</span></div>
      {live.length === 0
        ? <p className="side-empty">Nobody you follow is live.</p>
        : <ul className="side-channels">{live.map(user => <li key={user.username}><Link href={`/${user.username}`}><Crest faction={user.faction ?? null} initial={user.display_name.slice(0, 1).toUpperCase()} size={30} /><span className="side-channel-name">{user.display_name}</span><span className="live-dot" aria-hidden="true" /><span className="sr-only">, live</span></Link></li>)}</ul>}
    </section>}
  </>;

  return <html lang="en" data-theme={themeFor(account)} className={`${cinzel.variable} ${barlow.variable} ${barlowCondensed.variable}`}>
    <body>
      <SiteShell account={account} alerts={alerts} actions={actions} sidebar={sidebar}>{children}</SiteShell>
    </body>
  </html>;
}
