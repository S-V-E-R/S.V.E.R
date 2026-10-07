"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef } from "react";
import { FollowingIcon, SettingsIcon, ShieldIcon, StudioIcon, WalletIcon, HomeIcon } from "./Icons";

/**
 * The top-bar player menu (docs/DESIGN.md "Layout"): the player chip opens the account tools,
 * so the sidebar keeps to Home, Browse, War map and the faction hub.
 */
export function PlayerMenu({ username, chip }: { username: string; chip: React.ReactNode }) {
  const menu = useRef<HTMLDetailsElement>(null);
  const pathname = usePathname();
  useEffect(() => { if (menu.current) menu.current.open = false; }, [pathname]);
  useEffect(() => {
    const close = (event: MouseEvent | KeyboardEvent) => {
      const el = menu.current;
      if (!el?.open) return;
      if (event instanceof KeyboardEvent) { if (event.key === "Escape") { el.open = false; el.querySelector("summary")?.focus(); } return; }
      if (!el.contains(event.target as Node)) el.open = false;
    };
    document.addEventListener("click", close);
    document.addEventListener("keydown", close);
    return () => { document.removeEventListener("click", close); document.removeEventListener("keydown", close); };
  }, []);
  const items: [string, string, React.ReactNode][] = [
    [`/${username}`, "My channel", <HomeIcon key="c" />],
    ["/following", "Following", <FollowingIcon key="f" />],
    ["/studio/channel", "Creator Studio", <StudioIcon key="s" />],
    ["/wallet", "Valor", <WalletIcon key="v" />],
    ["/settings/profile", "Settings", <SettingsIcon key="t" />],
    ["/account", "Account security", <ShieldIcon key="a" />],
  ];
  return <details className="player-menu" ref={menu}>
    <summary className="player-chip" aria-label={`Account menu for ${username}`}>{chip}</summary>
    <nav className="player-menu-links panel" aria-label="Account">
      {items.map(([href, label, icon]) => <Link key={href} href={href} aria-current={pathname === href || (href !== `/${username}` && pathname?.startsWith(href + "/")) ? "page" : undefined}>{icon}{label}</Link>)}
    </nav>
  </details>;
}
