"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";

export function ChannelTabs({ username, live, tabs }: { username: string; live?: boolean; tabs: { wall: boolean; schedule: boolean; about: boolean; fan_art: boolean; videos?: boolean; beacons?: boolean } }) {
  const path = usePathname();
  // Followers and Following are the links in the counts row, not tabs (docs/PROFILES.md "Tabs").
  const items: [string, string, boolean][] = [["", "Home", true], ["/wall", "Wall", tabs.wall], ["/videos", "Videos", tabs.videos ?? true], ["/beacons", "Beacons", tabs.beacons ?? true], ["/schedule", "Schedule", tabs.schedule], ["/about", "About", tabs.about], ["/fan-art", "Fan art", tabs.fan_art]];
  return <nav className="channel-tabs" aria-label="Channel">{items.filter(i => i[2]).map(([href, label]) => {
    const target = `/${username}${href}`;
    // While live, plain /username opens the watch page; Home asks for the page itself.
    return <Link key={label} href={live && !href ? `${target}?home` : target} aria-current={path === target ? "page" : undefined}>{label}</Link>;
  })}</nav>;
}
