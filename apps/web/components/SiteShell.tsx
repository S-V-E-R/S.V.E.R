"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const siteLinks = [
  ["/about", "About"], ["/factions", "Factions"], ["/roadmap", "Roadmap"], ["/help", "Help & FAQ"], ["/terms", "Terms"],
  ["/privacy", "Privacy"], ["/guidelines", "Guidelines"],
  ["/dmca", "Copyright & DMCA"], ["/contact", "Contact"],
] as const;

export default function SiteShell({ account, alerts, children }: {
  account: { username: string } | null;
  alerts: boolean;
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const publicPage = siteLinks.some(([href]) => href === pathname);
  const navigation = <>
    <Link href="/" className="side-link">Home</Link>
    {account ? <>
      <Link href={`/${account.username}`} className="side-link">My channel</Link>
      <Link href="/following" className="side-link">Following</Link>
      <Link href="/studio/channel" className="side-link">Creator Studio</Link>
    </> : <><Link href="/login" className="side-link">Log in</Link><Link href="/signup" className="side-link">Enlist</Link></>}
    <Link href="/account" className="side-link">Account security</Link>
    <Link href="/about" className="side-link">About</Link>
    <Link href="/factions" className="side-link">Factions</Link>
    <Link href="/roadmap" className="side-link">Roadmap</Link>
    <Link href="/help" className="side-link">Help & FAQ</Link>
  </>;

  return <div className={publicPage ? "site-shell public-shell" : "site-shell"}>
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
        <Link href="/account" className="button small">Account security</Link>
      </> : <><Link href="/login">Log in</Link><Link href="/signup" className="button small">Enlist</Link></>}</nav>
      {!publicPage && <details className="mobile-navigation" key={pathname}>
        <summary>Menu</summary><nav aria-label="Mobile">{navigation}</nav>
      </details>}
    </header>
    <div className="workspace">
      {!publicPage && <aside className="sidebar"><nav aria-label="Your S.V.E.R">{navigation}</nav></aside>}
      <main id="main">{children}</main>
    </div>
    <footer className="site-footer">
      <div><Link href="/" className="brand" aria-label="S.V.E.R home">S.V.E.R</Link><p>For streamers who play, build, and make.</p></div>
      <nav className="site-links" aria-label="Site information">{siteLinks.map(([href, label]) =>
        <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{label}</Link>)}</nav>
      <p className="copyright">© {new Date().getFullYear()} SVER LLC</p>
    </footer>
  </div>;
}
