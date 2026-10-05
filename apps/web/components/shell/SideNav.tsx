"use client";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { BrowseIcon, FollowingIcon, HomeIcon, SettingsIcon, ShieldIcon, StudioIcon, WalletIcon, WarMapIcon } from "./Icons";

type Item = { href: string; label: string; icon: React.ReactNode; match: (path: string) => boolean };
const under = (base: string) => (path: string) => path === base || path.startsWith(`${base}/`);

/**
 * Main sidebar navigation (docs/DESIGN.md "Layout"). Only destinations that exist are listed:
 * Beacons joins this list when its module ships,
 * rather than sitting here greyed out.
 */
export function SideNav({ signedIn, faction }: { signedIn: boolean; faction: { name: string; slug: string } | null }) {
  const pathname = usePathname() ?? "/";
  const main: Item[] = [
    { href: "/", label: "Home", icon: <HomeIcon />, match: path => path === "/" },
    ...(signedIn ? [{ href: "/following", label: "Following", icon: <FollowingIcon />, match: under("/following") }] : []),
    {
      href: faction ? `/factions/${faction.slug}` : "/factions", label: faction ? `${faction.name} hub` : "Factions",
      icon: faction ? <Image src={`/factions/${faction.slug}.webp`} width={18} height={18} alt="" unoptimized /> : <WarMapIcon />,
      match: under("/factions")
    },
    { href: "/browse", label: "Browse", icon: <BrowseIcon />, match: under("/browse") },
    { href: "/magnet", label: "MAGNet", icon: <BrowseIcon />, match: under("/magnet") },
    { href: "/war-map", label: "War map", icon: <WarMapIcon />, match: under("/war-map") },
  ];
  const yours: Item[] = signedIn ? [
    { href: "/studio/channel", label: "Creator Studio", icon: <StudioIcon />, match: under("/studio") },
    { href: "/wallet", label: "Valor", icon: <WalletIcon />, match: under("/wallet") },
    { href: "/settings/profile", label: "Settings", icon: <SettingsIcon />, match: under("/settings") },
    { href: "/account", label: "Account security", icon: <ShieldIcon />, match: under("/account") },
  ] : [];
  const list = (items: Item[]) => <ul>{items.map(item => {
    const active = item.match(pathname);
    return <li key={item.href}><Link href={item.href} className="nav-item" aria-current={active ? "page" : undefined}>{item.icon}<span>{item.label}</span></Link></li>;
  })}</ul>;
  return <nav className="side-nav" aria-label="Main">
    {list(main)}
    {yours.length > 0 && <><span className="side-label nav-group">Your channel</span>{list(yours)}</>}
  </nav>;
}
