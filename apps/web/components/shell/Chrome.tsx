"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { CloseIcon, MenuIcon, SearchIcon } from "./Icons";

// Sign-in screens get no sidebar and a minimal top bar (docs/DESIGN.md "Pages": sign up, log in, …).
const AUTH_ROUTES = new Set(["login", "signup", "oauth-signup", "forgot", "reset", "verify", "mfa", "welcome", "choose-side", "choose-faction"]);

type Props = {
  /** Right side of the full top bar: notifications and player chip, or Log in and Enlist. */
  actions: React.ReactNode;
  sidebar: React.ReactNode;
  footer: React.ReactNode;
  children: React.ReactNode;
};

/**
 * The site shell: top bar, left sidebar (a drawer below 960 px) and the main column.
 * Client-side only for the pathname (auth screens) and the phone drawer; the parts inside are
 * rendered on the server and passed in.
 */
export function Chrome({ actions, sidebar, footer, children }: Props) {
  const pathname = usePathname() ?? "/";
  const first = pathname.split("/")[1] ?? "";
  // The drawer remembers the path it was opened on, so any navigation closes it without an effect.
  const [openOn, setOpenOn] = useState<string | null>(null);
  const open = openOn === pathname;
  const toggle = useRef<HTMLButtonElement>(null);
  const drawer = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!open) return;
    drawer.current?.querySelector<HTMLElement>("a[href], button")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpenOn(null);
      toggle.current?.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  function close() {
    setOpenOn(null);
    toggle.current?.focus();
  }

  if (AUTH_ROUTES.has(first)) {
    return <>
      <header className="topbar minimal">
        <Link href="/" className="logo">S.V.E.R</Link>
        <span className="topbar-space" />
        {/^(welcome|choose-side|choose-faction)$/.test(first) ? null : first === "login"
          ? <span className="topbar-note">New here? <Link href="/signup">Enlist</Link></span>
          : <span className="topbar-note">{first === "signup" ? "Already enlisted? " : ""}<Link href="/login">Log in</Link></span>}
      </header>
      <div className="workspace auth">
        <div className="content"><main id="main">{children}</main>{footer}</div>
      </div>
    </>;
  }

  return <>
    <header className="topbar">
      <button ref={toggle} type="button" className="menu-toggle" aria-controls="site-sidebar" aria-expanded={open} aria-label={open ? "Close menu" : "Open menu"} onClick={() => setOpenOn(open ? null : pathname)}>{open ? <CloseIcon /> : <MenuIcon />}</button>
      <Link href="/" className="logo">S.V.E.R</Link>
      <span className="topbar-space" />
      <div className="search">
        <SearchIcon />
        <label htmlFor="site-search" className="sr-only">Search channels, categories and Beacons</label>
        <input id="site-search" type="search" placeholder="Search · coming with MAGNet" disabled />
      </div>
      <span className="topbar-space" />
      <div className="topbar-actions">{actions}</div>
    </header>
    <div className="workspace">
      {open && <button type="button" className="drawer-backdrop" aria-label="Close menu" tabIndex={-1} onClick={close} />}
      <aside ref={drawer} id="site-sidebar" className={open ? "sidebar open" : "sidebar"} aria-label="Sidebar">{open && <button type="button" className="drawer-close" onClick={close}><CloseIcon />Close menu</button>}{sidebar}</aside>
      <div className="content"><main id="main">{children}</main>{footer}</div>
    </div>
  </>;
}
