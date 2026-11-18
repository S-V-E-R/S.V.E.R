"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect } from "react";
import { trackClicks } from "../lib/view-source";
import { Chrome } from "./shell/Chrome";
import { Logo, type LogoStyle } from "./LogoStyle";

const siteLinks = [
  ["/about", "About"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/help", "Help & FAQ"], ["/terms", "Terms"],
  ["/privacy", "Privacy"], ["/guidelines", "Guidelines"],
  ["/dmca", "Copyright & DMCA"], ["/contact", "Contact"],
  ["/take-it-down", "Take It Down requests"], ["/open-data", "Open data"], ["/developers", "Developers"], ["/credits", "Credits"],
] as const;

export default function SiteShell({ account, alerts, actions, sidebar, banner, children }: {
  account: { username: string; logo_style: LogoStyle } | null;
  alerts: boolean;
  actions: React.ReactNode;
  sidebar: React.ReactNode;
  /** Staff's site banner; never on embeds, overlays or pop-out chat. */
  banner?: React.ReactNode;
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  // Where views come from, for streamers' analytics (docs/ANALYTICS.md).
  useEffect(() => trackClicks(), []);
  // The last page before the sign-in screens, so logging in returns there (a stream, a Beacon)
  // rather than Home. Read by the login screen when the link carried no `next`.
  useEffect(() => {
    if (!pathname || /^\/(login|signup|oauth-signup|oauth|forgot|reset|verify|mfa|welcome|choose-side|choose-faction)(\/|$)/.test(pathname)) return;
    try { sessionStorage.setItem("return-to", pathname + window.location.search); } catch { /* storage blocked: login goes Home */ }
  }, [pathname]);
  if (pathname?.startsWith("/embed/")) return <main id="main">{children}</main>;
  // The OBS board overlay is a bare, transparent browser source.
  if (pathname?.startsWith("/overlay/")) return <>{children}</>;
  // Pop-out chat, its OBS dock and overlay, and the OBS Studio dock (/dock): only the page itself.
  if (pathname === "/dock" || (pathname && /^\/[^/]+\/chat$/.test(pathname))) return <main id="main">{children}</main>;
  const publicPage = siteLinks.some(([href]) => href === pathname);
  const footer = <SiteFooter />;
  if (!publicPage) return <><a className="skip" href="#main">Skip to content</a><Chrome logo={account?.logo_style} signedIn={!!account} actions={actions} sidebar={sidebar} footer={footer}>{banner}{children}</Chrome></>;

  return <div className="site-shell public-shell">
    <a className="skip" href="#main">Skip to content</a>
    <header className="topbar">
      <Logo style={account?.logo_style} className="brand" />
      <nav className="public-navigation" aria-label="Main">
        {[["/", "Home"], ["/browse", "Browse"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/about", "About"], ["/help", "Help"]].map(([href, label]) =>
          <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{label}</Link>)}
      </nav>
      <nav className="account-navigation" aria-label="Account">{account ? <>
        <Link href={`/${account.username}`} className="account-name">@{account.username}</Link>
        <Link href="/settings/profile">Settings{alerts && <><span className="nav-dot" aria-hidden="true" /><span className="sr-only"> (new notices)</span></>}</Link>
        <Link href="/account">Account security</Link>
      </> : <><Link href="/login">Log in</Link><Link href="/signup" className="button small">Enlist</Link></>}</nav>
    </header>
    <div className="workspace">
      <main id="main">{banner}{children}</main>
    </div>
    {footer}
  </div>;
}

// Every footer destination, grouped; Take It Down stays clearly visible (docs/DESIGN.md "Footer").
const footerColumns = [
  ["S.V.E.R", [["/about", "About"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/guilds", "Guilds"], ["/wall", "The Wall"], ["/supporters", "Support S.V.E.R"]]],
  ["Help & data", [["/help", "Help & FAQ"], ["/contact", "Contact"], ["/open-data", "Open data"], ["/developers", "Developers"], ["/crowdsync", "CrowdSync catalog"], ["/credits", "Credits"]]],
  ["Rules", [["/terms", "Terms"], ["/privacy", "Privacy"], ["/guidelines", "Guidelines"], ["/dmca", "Copyright & DMCA"], ["/take-it-down", "Take It Down requests"]]],
] as const;
function SiteFooter() {
  const pathname = usePathname();
  return <footer className="site-footer">
      <div><Link href="/" className="brand logo small" aria-label="S.V.E.R home">S.V.E.R</Link><p>Your people are here. Your destiny is yours to forge. Choose your faction.</p></div>
      <nav className="footer-columns" aria-label="Site information">{footerColumns.map(([title, links]) => <div key={title}><span className="eyebrow">{title}</span>{links.map(([href, label]) =>
        <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{label}</Link>)}</div>)}</nav>
      <p className="copyright">© {new Date().getFullYear()} SVER LLC <span className="verse">Matthew 5:16</span></p>
    </footer>;
}
