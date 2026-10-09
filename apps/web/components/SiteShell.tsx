"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Chrome } from "./shell/Chrome";

const siteLinks = [
  ["/about", "About"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/help", "Help & FAQ"], ["/terms", "Terms"],
  ["/privacy", "Privacy"], ["/guidelines", "Guidelines"],
  ["/dmca", "Copyright & DMCA"], ["/contact", "Contact"],
  ["/take-it-down", "Take It Down requests"],
] as const;

export default function SiteShell({ account, alerts, actions, sidebar, children }: {
  account: { username: string } | null;
  alerts: boolean;
  actions: React.ReactNode;
  sidebar: React.ReactNode;
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  if (pathname?.startsWith("/embed/")) return <main id="main">{children}</main>;
  // The OBS board overlay is a bare, transparent browser source.
  if (pathname?.startsWith("/overlay/")) return <>{children}</>;
  // Pop-out chat, its OBS dock and overlay: only the chat (docs/CHANNEL_ADDITIONS.md "Pop-out chat").
  if (pathname && /^\/[^/]+\/chat$/.test(pathname)) return <main id="main">{children}</main>;
  const publicPage = siteLinks.some(([href]) => href === pathname);
  const footer = <SiteFooter />;
  if (!publicPage) return <><a className="skip" href="#main">Skip to content</a><Chrome actions={actions} sidebar={sidebar} footer={footer}>{children}</Chrome></>;

  return <div className="site-shell public-shell">
    <a className="skip" href="#main">Skip to content</a>
    <header className="topbar">
      <Link href="/" className="brand" aria-label="S.V.E.R home">S.V.E.R</Link>
      <nav className="public-navigation" aria-label="Main">
        {[["/", "Home"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/about", "About"], ["/help", "Help"]].map(([href, label]) =>
          <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{label}</Link>)}
      </nav>
      <nav className="account-navigation" aria-label="Account">{account ? <>
        <Link href={`/${account.username}`} className="account-name">@{account.username}</Link>
        <Link href="/settings/profile">Settings{alerts && <span className="nav-dot" aria-label="New notices" />}</Link>
        <Link href="/account">Account security</Link>
      </> : <><Link href="/login">Log in</Link><Link href="/signup" className="button small">Enlist</Link></>}</nav>
    </header>
    <div className="workspace">
      <main id="main">{children}</main>
    </div>
    {footer}
  </div>;
}

function SiteFooter() {
  const pathname = usePathname();
  return <footer className="site-footer">
      <div><Link href="/" className="brand logo small" aria-label="S.V.E.R home">S.V.E.R</Link><p>Your people are here. Your destiny is yours to forge. Choose your faction.</p></div>
      <nav className="site-links" aria-label="Site information">{siteLinks.map(([href, label]) =>
        <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{label}</Link>)}</nav>
      <p className="copyright">© {new Date().getFullYear()} SVER LLC <span className="verse">Matthew 5:16</span></p>
    </footer>;
}
