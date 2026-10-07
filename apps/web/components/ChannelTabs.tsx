"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";

export function ChannelTabs({ username, tabs }: { username: string; tabs: { wall: boolean; schedule: boolean; about: boolean; fan_art: boolean } }) {
  const path = usePathname();
  const items: [string, string, boolean][] = [["", "Home", true], ["/videos", "Videos", true], ["/beacons", "Beacons", true], ["/wall", "Wall", tabs.wall], ["/schedule", "Schedule", true], ["/about", "About", tabs.about], ["/fan-art", "Fan art", tabs.fan_art], ["/followers", "Followers", true], ["/following", "Following", true]];
  return <nav className="channel-tabs" aria-label="Channel">{items.filter(i => i[2]).map(([href, label]) => {
    const target = `/${username}${href}`;
    return <Link key={label} href={target} aria-current={path === target ? "page" : undefined}>{label}</Link>;
  })}</nav>;
}
