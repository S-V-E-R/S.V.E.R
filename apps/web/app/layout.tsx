import type { Metadata } from "next";
import { Barlow, Barlow_Condensed, Cinzel } from "next/font/google";
import Link from "next/link";
import { Chrome } from "../components/shell/Chrome";
import { BellIcon, MagnetMark } from "../components/shell/Icons";
import { SideNav } from "../components/shell/SideNav";
import type { PeoplePage } from "../components/People";
import { apiGet } from "../lib/server-api";
import { themeFor } from "../lib/theme";
import { currentAccount, hasAlerts } from "./session";
import "./globals.css";
import "../styles/profiles.css";

// Type per docs/DESIGN.md "Type": self-hosted and subset by next/font.
const cinzel = Cinzel({ subsets: ["latin"], weight: ["700", "800"], variable: "--font-cinzel", display: "swap" });
const barlow = Barlow({ subsets: ["latin"], weight: ["400", "500", "600"], variable: "--font-barlow", display: "swap" });
const barlowCondensed = Barlow_Condensed({ subsets: ["latin"], weight: ["600", "700"], variable: "--font-barlow-condensed", display: "swap" });

export const metadata: Metadata = { title: "S.V.E.R", description: "Live streaming for people who play, build, and make.", robots: { index: false, follow: false } };

/** Channels the viewer follows that are live now, from the first page of their follows. */
async function followingLive() {
  const page = (await apiGet<PeoplePage>("/api/me/following")).data;
  return (page?.items ?? []).map(item => item.user).filter(user => user.live && user.username);
}

export default async function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  const account = await currentAccount();
  const [alerts, live] = account ? await Promise.all([hasAlerts(), followingLive()]) : [false, []];
  const initial = account?.username.slice(0, 1).toUpperCase();

  const actions = account
    ? <>
      <Link href="/settings/profile" className="icon-button" aria-label={alerts ? "Notifications, new notices" : "Notifications"}><BellIcon />{alerts && <span className="alert-badge" aria-hidden="true" />}</Link>
      <Link href={`/${account.username}`} className="player-chip">
        {/* Crest placeholder until Module 4 gives the account a faction. */}
        <span className="crest-slot" aria-hidden="true">{initial}</span>
        <span className="player-chip-name">@{account.username}</span>
      </Link>
    </>
    : <>
      <Link href="/login" className="topbar-link">Log in</Link>
      <Link href="/signup" className="button">Enlist</Link>
    </>;

  const sidebar = <>
    {account && <Link href={`/${account.username}`} className="player-card frame">
      <span className="crest-slot large" aria-hidden="true">{initial}</span>
      <span className="player-card-text"><span className="player-card-name">{account.username}</span><span className="player-card-faction">No faction yet</span></span>
    </Link>}
    <SideNav signedIn={!!account} />
    {account && <section className="side-section" aria-labelledby="following-live">
      <div className="side-label"><span id="following-live">Following · live</span><span className="magnet"><MagnetMark />MAGNet</span></div>
      {live.length === 0
        ? <p className="side-empty">Nobody you follow is live.</p>
        : <ul className="side-channels">{live.map(user => <li key={user.username}><Link href={`/${user.username}`}><span className="crest-slot small" aria-hidden="true">{user.display_name.slice(0, 1).toUpperCase()}</span><span className="side-channel-name">{user.display_name}</span><span className="live-dot" aria-hidden="true" /><span className="sr-only">, live</span></Link></li>)}</ul>}
    </section>}
  </>;

  // TODO(policy pages): add footer links to About, Factions, Roadmap, Help, Terms, Privacy, Guidelines,
  // DMCA, Take It Down requests and Contact as each page lands (docs/DESIGN.md "Footer"). Take It Down
  // requests must be clearly visible from the homepage. None of these routes exist yet, so no links.
  const footer = <footer className="site-footer">
    <div className="footer-brand"><span className="logo small">S.V.E.R</span><span>For streamers who play, build, and make.</span></div>
    <span>© {new Date().getFullYear()} SVER LLC</span>
  </footer>;

  return <html lang="en" data-theme={themeFor(account)} className={`${cinzel.variable} ${barlow.variable} ${barlowCondensed.variable}`}>
    <body>
      <a className="skip" href="#main">Skip to content</a>
      <Chrome actions={actions} sidebar={sidebar} footer={footer}>{children}</Chrome>
    </body>
  </html>;
}
