"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BeaconsIcon, BrowseIcon, FactionIcon, HomeIcon, WarMapIcon } from "./Icons";

type Item = { href?: string; label: string; icon: React.ReactNode; upcoming?: string };

const ITEMS: Item[] = [
  { href: "/", label: "Home", icon: <HomeIcon /> },
  { label: "Browse", icon: <BrowseIcon />, upcoming: "Coming with MAGNet" },
  { label: "Beacons", icon: <BeaconsIcon />, upcoming: "Coming with Beacons" },
  { label: "War map", icon: <WarMapIcon />, upcoming: "Coming with Factions" },
  { label: "Faction hub", icon: <FactionIcon />, upcoming: "Coming with Factions" }
];

/** Main sidebar navigation. A client component only so the active item can follow the pathname. */
export function SideNav() {
  const pathname = usePathname() ?? "/";
  return <nav className="side-nav" aria-label="Main">
    <ul>
      {ITEMS.map(item => <li key={item.label}>{item.href
        ? <Link href={item.href} className="nav-item" aria-current={pathname === item.href ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link>
        : <span className="nav-item soon" aria-disabled="true" title={item.upcoming}>{item.icon}<span>{item.label}</span><span className="soon-chip">Soon</span><span className="sr-only">{item.upcoming}</span></span>}
      </li>)}
    </ul>
  </nav>;
}
