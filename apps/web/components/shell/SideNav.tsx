"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BeaconsIcon, BrowseIcon, FactionIcon, HomeIcon, WarMapIcon } from "./Icons";

import { factionInfo, type Faction } from "../../lib/factions";
import { Crest } from "../FactionIdentity";

type Item = { href?: string; label: string; icon: React.ReactNode; upcoming?: string };

const ITEMS: Item[] = [
  { href: "/", label: "Home", icon: <HomeIcon /> },
  { label: "Browse", icon: <BrowseIcon />, upcoming: "Coming with MAGNet" },
  { label: "Beacons", icon: <BeaconsIcon />, upcoming: "Coming with Beacons" },
  { href: "/war-map", label: "War map", icon: <WarMapIcon /> },

];

/** Main sidebar navigation. A client component only so the active item can follow the pathname. */
export function SideNav({ faction = null }: { faction?: Faction | null }) {
  const items: Item[] = [...ITEMS, { href: faction ? `/factions/${faction}` : "/choose-faction", label: faction ? `${factionInfo(faction).name} hub` : "Choose your side", icon: faction ? <Crest faction={faction} size={18} /> : <FactionIcon /> }];
  const pathname = usePathname() ?? "/";
  return <nav className="side-nav" aria-label="Main">
    <ul>
      {items.map(item => <li key={item.label}>{item.href
        ? <Link href={item.href} className="nav-item" aria-current={pathname === item.href ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link>
        : <span className="nav-item soon" aria-disabled="true" title={item.upcoming}>{item.icon}<span>{item.label}</span><span className="soon-chip">Soon</span><span className="sr-only">{item.upcoming}</span></span>}
      </li>)}
    </ul>
  </nav>;
}
