import type { Metadata } from "next";
import { Barlow, Barlow_Condensed, Cinzel } from "next/font/google";
import Link from "next/link";
import SiteShell from "../components/SiteShell";
import { StaffRemovalAlerts } from "../components/StaffRemovalAlerts";
import { BellIcon, MagnetMark } from "../components/shell/Icons";
import { SideNav } from "../components/shell/SideNav";
import { PlayerMenu } from "../components/shell/PlayerMenu";
import { Avatar } from "../components/Avatar";
import type { Chip } from "../lib/types";
import type { PeoplePage } from "../components/People";
import { apiGet } from "../lib/server-api";
import { themeFor } from "../lib/theme";
import { currentAccount, hasAlerts } from "./session";
import "./globals.css";
import "../styles/profiles.css";
import "../styles/design.css";
import "../styles/site-pages.css";
import "../styles/discovery.css";
import "../styles/factions.css";
import { Crest } from "../components/FactionIdentity";
import { factionInfo } from "../lib/factions";
import type { StreamDirectory } from "../components/StreamShelf";

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
  const page = (await apiGet<PeoplePage>("/api/me/following")).data;
  if (!page) return null;
  const users = page.items.map(item => item.user).filter(user => user.live && user.username).slice(0, 8);
  const channels = await Promise.all(users.map(async user => {
    const state = (await apiGet<{ live: boolean; category?: string | null; viewers?: number }>(`/api/channels/${encodeURIComponent(user.username!)}/live`)).data;
    return state?.live ? { user, category: state.category ?? null, viewers: state.viewers ?? null } : null;
  }));
  return channels.filter(channel => channel !== null);
}

async function pickedLive() {
  const page = (await apiGet<StreamDirectory>("/api/streams")).data;
  return page?.live.slice(0, 8).map(({ user, category, viewers }) => ({ user, category, viewers })) ?? null;
}

export default async function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  const account = await currentAccount();
  const [alerts, live, profile] = await Promise.all([
    account ? hasAlerts() : false,
    account ? followingLive() : pickedLive(),
    account ? apiGet<Pick<Chip, "display_name" | "avatar">>(`/api/users/${encodeURIComponent(account.username)}/card`) : null,
  ]);
  const avatar = profile?.data?.avatar ?? null;
  const displayName = profile?.data?.display_name ?? account?.username ?? "";

  const actions = account
    ? <>
      <StaffRemovalAlerts />
      <Link href="/notifications" className="icon-button" aria-label={alerts ? "Notifications, new notices" : "Notifications"}><BellIcon />{alerts && <span className="alert-badge" aria-hidden="true" />}</Link>
      <PlayerMenu username={account.username} avatar={avatar} faction={account.faction} />
    </>
    : <>
      <Link href="/login" className="topbar-link">Log in</Link>
      <Link href="/signup" className="button">Enlist</Link>
    </>;

  const sidebar = <>
    {account && <Link href={`/${account.username}`} className="player-card frame">
      {account.faction ? <Crest faction={account.faction} size={48} /> : <Avatar sizes={avatar} name={displayName} size={48} />}
      <span className="player-card-text"><span className="player-card-name">{displayName}</span><span className="player-card-faction">{account.faction ? `${factionInfo(account.faction).name} · ${factionInfo(account.faction).title}` : "No faction yet"}</span><span className="player-card-link">View your channel</span></span>
    </Link>}
    <SideNav faction={account?.faction ?? null} />
    <section className="daily-orders frame" aria-labelledby="daily-orders-title"><h2 id="daily-orders-title">Daily orders</h2><p>Watch, chat and help your faction.</p><p className="daily-orders-status">Coming with Progression</p><Link href="/roadmap">See the roadmap</Link></section>
    <section className="side-section" aria-labelledby="following-live">
      <div className="side-label"><span id="following-live">{account ? "Following · live" : "Picked for you"}</span><span className="magnet"><MagnetMark />MAGNet</span></div>
      {!live?.length
        ? <p className="side-empty">{!live ? "Live channels are unavailable." : account ? "Nobody you follow is live." : "Nothing live right now."}</p>
        : <ul className="side-channels">{live.map(({ user, category, viewers }) => <li key={user.username}><Link href={`/${user.username}/live`}><Avatar sizes={user.avatar} name={user.display_name} size={28} /><span className="side-channel-text"><span className="side-channel-name">{user.display_name}</span>{category && <span className="side-channel-category">{category}</span>}</span><span className="side-channel-count"><span className="live-dot" aria-hidden="true" />{viewers !== null && viewers.toLocaleString()}<span className="sr-only"> watching live</span></span></Link></li>)}</ul>}
    </section>
  </>;

  return <html lang="en" data-theme={themeFor(account)} className={`${cinzel.variable} ${barlow.variable} ${barlowCondensed.variable}`}>
    <body>
      <SiteShell account={account} alerts={alerts} actions={actions} sidebar={sidebar}>{children}</SiteShell>
    </body>
  </html>;
}
