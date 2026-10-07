"use client";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BeaconsIcon, BrowseIcon, FactionIcon, HomeIcon, MagnetMark, WarMapIcon } from "./Icons";

type Item = { href: string; label: string; icon: React.ReactNode; match: (path: string) => boolean };
const under = (base: string) => (path: string) => path === base || path.startsWith(`${base}/`);

/**
 * Main sidebar navigation (docs/DESIGN.md "Layout"): Home, Browse, MAGNet, Beacons, War map, then
 * the viewer's faction hub. Account tools are in the top-bar player menu.
 */
export function SideNav({ faction }: { faction: { name: string; slug: string } | null }) {
  const pathname = usePathname() ?? "/";
  const main: Item[] = [
    { href: "/", label: "Home", icon: <HomeIcon />, match: path => path === "/" },
    { href: "/browse", label: "Browse", icon: <BrowseIcon />, match: under("/browse") },
    { href: "/magnet", label: "MAGNet", icon: <span className="nav-magnet"><MagnetMark /></span>, match: under("/magnet") },
    { href: "/beacons", label: "Beacons", icon: <BeaconsIcon />, match: under("/beacons") },
    { href: "/war-map", label: "War map", icon: <WarMapIcon />, match: under("/war-map") },
    {
      href: faction ? `/factions/${faction.slug}` : "/factions", label: faction ? `${faction.name} hub` : "Factions",
      icon: faction ? <Image src={`/factions/${faction.slug}.webp`} width={18} height={18} alt="" unoptimized /> : <FactionIcon />,
      match: under("/factions")
    },
  ];
  return <nav className="side-nav" aria-label="Main"><ul>{main.map(item => {
    const active = item.match(pathname);
    return <li key={item.href}><Link href={item.href} className="nav-item" aria-current={active ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link></li>;
  })}</ul></nav>;
}
