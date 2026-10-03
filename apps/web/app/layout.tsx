import type { Metadata } from "next";
import { Barlow, Barlow_Condensed, Cinzel } from "next/font/google";
import { currentAccount, hasAlerts } from "./session";
import SiteShell from "../components/SiteShell";
import "./globals.css";
import "../styles/profiles.css";
import "../styles/design.css";
import "../styles/site-pages.css";

const heading = Cinzel({ subsets: ["latin"], weight: ["700", "800"], variable: "--font-heading", display: "swap" });
const body = Barlow({ subsets: ["latin"], weight: ["400", "500", "600"], variable: "--font-body", display: "swap" });
const label = Barlow_Condensed({ subsets: ["latin"], weight: ["600", "700"], variable: "--font-label", display: "swap" });

export const metadata: Metadata = { title: "S.V.E.R", description: "Live streaming for people who play, build, and make.", robots: { index: false, follow: false } };
export default async function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  const account = await currentAccount();
  const alerts = account ? await hasAlerts() : false;
  // Faction membership arrives with Module 4; accounts currently use the neutral theme.
  return <html lang="en" data-theme="neutral" className={`${heading.variable} ${body.variable} ${label.variable}`}>
    <body><SiteShell account={account} alerts={alerts}>{children}</SiteShell></body>
  </html>;
}
