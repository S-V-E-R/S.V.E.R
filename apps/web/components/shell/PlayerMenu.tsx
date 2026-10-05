"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Crest } from "../FactionIdentity";
import type { Faction } from "../../lib/factions";
import { Avatar } from "../Avatar";
import type { Sizes } from "../../lib/types";
import { FollowingIcon, HomeIcon, SettingsIcon, ShieldIcon, StudioIcon } from "./Icons";

/** Account tools belong in the player menu; the sidebar is for discovery. */
export function PlayerMenu({ username, avatar, faction }: { username: string; avatar: Sizes; faction?: Faction | null }) {
  const pathname = usePathname();
  const links = [[`/${username}`, "My channel", <HomeIcon key="home" />], ["/following", "Following", <FollowingIcon key="following" />], ["/studio/channel", "Creator Studio", <StudioIcon key="studio" />], ["/settings/profile", "Settings", <SettingsIcon key="settings" />], ["/choose-faction", "Your faction", <ShieldIcon key="faction" />], ["/account", "Account security", <ShieldIcon key="security" />]] as const;
  return <details key={pathname} className="player-menu" onKeyDown={event => {
    if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); }
  }}>
    <summary className="player-chip" aria-label={`Account menu for @${username}`}>{faction ? <Crest faction={faction} size={32} /> : <Avatar sizes={avatar} name={username} size={32} />}<span className="player-chip-name">@{username}</span><svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" aria-hidden="true"><path d="m2 4 4 4 4-4" /></svg></summary>
    <nav className="player-menu-links frame" aria-label="Account tools">{links.map(([href, label, icon]) => <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined} onClick={event => { const menu = event.currentTarget.closest("details"); if (menu) menu.open = false; }}>{icon}{label}</Link>)}</nav>
  </details>;
}
