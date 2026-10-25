"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";

export type MenuGroup = [string | null, [string, string][]];

/**
 * The left-hand menu for Creator Studio and Settings (docs/DESIGN.md "Side menus"). The current page is
 * marked; on phones the list folds behind one button that names the current page, instead of a wall of links.
 */
export function SideMenu({ label, groups, footer }: { label: string; groups: MenuGroup[]; footer?: React.ReactNode }) {
  const pathname = usePathname() ?? "";
  // Remembers which page the menu was opened on, so moving to another page closes it.
  const [openOn, setOpenOn] = useState<string | null>(null);
  const open = openOn === pathname;
  const current = groups.flatMap(([, links]) => links).find(([href]) => href === pathname)?.[1];
  return <nav className={groups.some(([g]) => g) ? "settings-nav studio-nav" : "settings-nav"} aria-label={label}>
    <span className="eyebrow">{label}</span>
    <button type="button" className="side-menu-toggle quiet" aria-expanded={open} aria-controls="side-menu-links" onClick={() => setOpenOn(open ? null : pathname)}>
      <span>{current ?? "Menu"}</span><svg aria-hidden="true" width="14" height="14" viewBox="0 0 14 14"><path d={open ? "M3 9l4-4 4 4" : "M3 5l4 4 4-4"} fill="none" stroke="currentColor" strokeWidth="1.6" /></svg>
    </button>
    <div id="side-menu-links" className={open ? "side-menu-links open" : "side-menu-links"}>
      {groups.map(([group, links]) => {
        const items = links.map(([href, text]) => <Link key={href} href={href} aria-current={pathname === href ? "page" : undefined}>{text}</Link>);
        // Labelled groups fold; the one holding the current page starts open, so the menu reads as a few headings.
        return group
          ? <details key={group} className="studio-nav-group" open={links.some(([href]) => href === pathname)}><summary className="studio-nav-label">{group}</summary>{items}</details>
          : <div key="links" className="side-menu-group">{items}</div>;
      })}
      {footer}
    </div>
  </nav>;
}
