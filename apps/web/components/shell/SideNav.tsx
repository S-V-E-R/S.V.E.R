"use client";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BeaconsIcon, BrowseIcon, FactionIcon, FollowingIcon, HomeIcon, MagnetMark, WarMapIcon } from "./Icons";

type Item = { href: string; label: string; icon: React.ReactNode; match: (path: string) => boolean };
const under = (base: string) => (path: string) => path === base || path.startsWith(`${base}/`);
const HOME: Item = { href: "/", label: "Home", icon: <HomeIcon />, match: path => path === "/" };
const FOLLOWING: Item = { href: "/following", label: "Following", icon: <FollowingIcon />, match: under("/following") };
const PLACES: Item[] = [
  { href: "/browse", label: "Browse", icon: <BrowseIcon />, match: under("/browse") },
  { href: "/magnet", label: "MAGNet", icon: <span className="nav-magnet"><MagnetMark /></span>, match: under("/magnet") },
  { href: "/beacons", label: "Beacons", icon: <BeaconsIcon />, match: under("/beacons") },
];

/**
 * Phones (below 960 px): the main places as a bar along the bottom, one tap from any page. War
 * map and the faction hub stay in the drawer.
 */
export function BottomNav({ signedIn }: { signedIn: boolean }) {
  const pathname = usePathname() ?? "/";
  return <nav className="bottom-nav" aria-label="Main places">{[HOME, ...PLACES, ...(signedIn ? [FOLLOWING] : [])].map(item =>
    <Link key={item.href} href={item.href} aria-current={item.match(pathname) ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link>)}</nav>;
}

/**
 * Main sidebar navigation (docs/DESIGN.md "Layout"): Home, Following (signed in, with how many
 * followed channels are live), Browse, MAGNet, Beacons, War map, then the viewer's faction hub.
 * Account tools are in the top-bar player menu.
 */
export function SideNav({ faction, signedIn = false, followingLive = 0 }: { faction: { name: string; slug: string } | null; signedIn?: boolean; followingLive?: number }) {
  const pathname = usePathname() ?? "/";
  const main: Item[] = [
    HOME,
    ...(signedIn ? [FOLLOWING] : []),
    ...PLACES,
    { href: "/war-map", label: "War map", icon: <WarMapIcon />, match: under("/war-map") },
    {
      href: faction ? `/factions/${faction.slug}` : "/factions", label: faction ? `${faction.name} hub` : "Factions",
      icon: faction ? <Image src={`/factions/${faction.slug}.webp`} width={18} height={18} alt="" unoptimized /> : <FactionIcon />,
      match: under("/factions")
    },
  ];
  return <nav className="side-nav" aria-label="Main"><ul>{main.map(item => {
    const active = item.match(pathname);
    return <li key={item.href}><Link href={item.href} className="nav-item" aria-current={active ? "page" : undefined}>{item.icon}<span>{item.label}</span>{item === FOLLOWING && followingLive > 0 && <span className="side-channel-count nav-count"><span className="live-dot" aria-hidden="true" />{followingLive}<span className="sr-only"> live</span></span>}</Link></li>;
  })}</ul></nav>;
}
