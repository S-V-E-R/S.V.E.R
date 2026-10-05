"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Avatar } from "../Avatar";
import type { Sizes } from "../../lib/types";
import { FollowingIcon, HomeIcon, SettingsIcon, ShieldIcon, StudioIcon } from "./Icons";

/** Account tools belong in the player menu; the sidebar is for discovery. */
export function PlayerMenu({ username, avatar }: { username: string; avatar: Sizes }) {
  const pathname = usePathname();
  const links = [[`/${username}`, "My channel", <HomeIcon key="home" />], ["/following", "Following", <FollowingIcon key="following" />], ["/studio/channel", "Creator Studio", <StudioIcon key="studio" />], ["/settings/profile", "Settings", <SettingsIcon key="settings" />], ["/account", "Account security", <ShieldIcon key="security" />]] as const;
  return <details key={pathname} className="player-menu" onKeyDown={event => {
    if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); }
  }}>
    <summary className="player-chip" aria-label={`Account menu for @${username}`}><Avatar sizes={avatar} name={username} size={32} /><span className="player-chip-name">@{username}</span><svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" aria-hidden="true"><path d="m2 4 4 4 4-4" /></svg></summary>
    <nav className="player-menu-links frame" aria-label="Account tools">{links.map(([href, label, icon]) => <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined} onClick={event => { const menu = event.currentTarget.closest("details"); if (menu) menu.open = false; }}>{icon}{label}</Link>)}</nav>
  </details>;
}
