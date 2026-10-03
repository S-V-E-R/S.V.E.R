"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BeaconsIcon, BrowseIcon, FactionIcon, FollowingIcon, HomeIcon, SettingsIcon, ShieldIcon, StudioIcon, WarMapIcon } from "./Icons";

type Item = { href: string; label: string; icon: React.ReactNode; match: (path: string) => boolean; signedIn?: boolean };
const under = (base: string) => (path: string) => path === base || path.startsWith(`${base}/`);

const ITEMS: Item[] = [
  { href: "/", label: "Home", icon: <HomeIcon />, match: path => path === "/" },
  { href: "/following", label: "Following", icon: <FollowingIcon />, match: under("/following"), signedIn: true },
  { href: "/studio/channel", label: "Creator Studio", icon: <StudioIcon />, match: under("/studio"), signedIn: true },
  { href: "/settings/profile", label: "Settings", icon: <SettingsIcon />, match: under("/settings"), signedIn: true },
  { href: "/account", label: "Account security", icon: <ShieldIcon />, match: under("/account"), signedIn: true }
];

// Destinations from docs/DESIGN.md that aren't built yet. They are shown, but never linked,
// until their routes exist (Browse and the war map with Modules 4–5, Beacons with Module 7).
const SOON = [
  { label: "Browse", icon: <BrowseIcon /> },
  { label: "Beacons", icon: <BeaconsIcon /> },
  { label: "War map", icon: <WarMapIcon /> },
  { label: "Faction hub", icon: <FactionIcon /> }
];

/** Main sidebar navigation. A client component only so the active item can follow the pathname. */
export function SideNav({ signedIn }: { signedIn: boolean }) {
  const pathname = usePathname() ?? "/";
  return <nav className="side-nav" aria-label="Main">
    <ul>
      {ITEMS.filter(item => signedIn || !item.signedIn).map(item => {
        const active = item.match(pathname);
        return <li key={item.href}><Link href={item.href} className="nav-item" aria-current={active ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link></li>;
      })}
      {SOON.map(item => <li key={item.label}><span className="nav-item soon" aria-disabled="true">{item.icon}<span>{item.label}</span><span className="soon-chip">Soon</span></span></li>)}
    </ul>
  </nav>;
}
