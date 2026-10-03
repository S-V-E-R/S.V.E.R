import type { Metadata } from "next";
import Link from "next/link";
import { currentAccount, hasAlerts } from "./session";
import "./globals.css";
import "../styles/profiles.css";

export const metadata: Metadata = { title: "S.V.E.R — Your next chapter", description: "Your account. Your identity. Your place on S.V.E.R.", robots: { index: false, follow: false } };
export default async function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  const account = await currentAccount();
  const alerts = account ? await hasAlerts() : false;
  return <html lang="en"><body data-theme="steel"><a className="skip" href="#main">Skip to content</a>
    <header className="topbar"><Link href="/" className="brand" aria-label="S.V.E.R home"><span className="brand-mark" aria-hidden="true">S</span><span>S.V.E.R<span className="brand-caption">STREAM • CONNECT • BELONG</span></span></Link><div className="topline">A NEW CHAPTER BEGINS</div><nav aria-label="Account">{account ? <><Link href={`/${account.username}`} className="account-name">@{account.username}</Link><Link href="/settings/profile">Settings{alerts && <span className="nav-dot" aria-label="New notices" />}</Link><Link href="/account" className="button small">Account security <span aria-hidden="true">↗</span></Link></> : <><Link href="/login">Sign in</Link><Link href="/signup" className="button small">Create account <span aria-hidden="true">↗</span></Link></>}</nav></header>
    <div className="workspace"><aside className="sidebar"><div className="side-label">COMMAND</div>{!account && <><Link href="/login" className="side-link"><span aria-hidden="true">⌁</span> Sign in <span aria-hidden="true">›</span></Link><Link href="/signup" className="side-link"><span aria-hidden="true">⊕</span> Create account <span aria-hidden="true">›</span></Link></>}{account && <><Link href={`/${account.username}`} className="side-link"><span aria-hidden="true">◈</span> My channel <span aria-hidden="true">›</span></Link><Link href="/following" className="side-link"><span aria-hidden="true">✦</span> Following <span aria-hidden="true">›</span></Link><Link href="/studio/channel" className="side-link"><span aria-hidden="true">▣</span> Creator Studio <span aria-hidden="true">›</span></Link></>}<Link href="/account" className="side-link"><span aria-hidden="true">◇</span> Account security <span aria-hidden="true">›</span></Link><div className="side-rule"/><p className="side-note">One identity.<br/>Every side of you.</p><div className="side-bottom"><span className="status-dot"/> S.V.E.R 2.0 <span className="muted">/ LOGIN</span></div></aside>
      <main id="main">{children}</main></div><footer><span>© {new Date().getFullYear()} S.V.E.R</span><span>BUILT FOR THE PEOPLE WHO SHOW UP.</span><Link href="/account">Account & privacy</Link></footer>
  </body></html>;
}
