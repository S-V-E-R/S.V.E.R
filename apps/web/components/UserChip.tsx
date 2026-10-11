"use client";
import Link from "next/link";
import { type CSSProperties, type MouseEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { send } from "../lib/client-api";
import { joined, platformNames, type Chip, type Sizes } from "../lib/types";
import { Crest } from "./FactionIdentity";
import { Avatar } from "./Avatar";

type Card = { username: string; display_name: string; avatar: Sizes; bio: string; follower_count: number; joined_at: string; faction?: Chip["faction"]; viewer: { signed_in: boolean; is_self: boolean; following: boolean; blocked: boolean; can_message?: boolean }; live: boolean; level?: number; title?: string; frame?: number; showcase?: { key: string; name: string }[]; also_known_as?: { platform: string; handle: string; url: string | null }[] };

/**
 * A button that opens someone's player card (docs/PROFILES.md, "User card"): used by name chips,
 * chat names and anywhere else a name is shown. The card is fixed to the screen, so a scrolling
 * list like chat never clips it; on phones it's a bottom sheet.
 */
export function PlayerCardButton({ username, className, title, data, children }: { username: string; className?: string; title?: string; data?: Record<string, string | undefined>; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [card, setCard] = useState<Card | null>(null);
  const [error, setError] = useState("");
  const [place, setPlace] = useState<CSSProperties>({});
  const root = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent ? event.key === "Escape" : !root.current?.contains(event.target as Node)) setOpen(false);
    };
    // A card fixed to the screen would drift from its name, so scrolling anything else closes it.
    const scrolled = (event: Event) => { if (!root.current?.querySelector(".user-card")?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    window.addEventListener("scroll", scrolled, true);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", close); window.removeEventListener("scroll", scrolled, true); };
  }, [open]);
  async function load() {
    const result = await send<Card>("GET", `/api/users/${encodeURIComponent(username)}/card`);
    if (result.ok) setCard(result.data); else setError(result.error);
  }
  async function toggle(event: MouseEvent<HTMLButtonElement>) {
    if (!open) {
      // Desktop: next to the name, flipped above it near the bottom of the screen.
      const at = event.currentTarget.getBoundingClientRect();
      const width = 300, height = 340;
      setPlace(window.innerWidth > 650 ? {
        position: "fixed",
        left: Math.max(8, Math.min(at.left, window.innerWidth - width - 8)),
        top: at.bottom + height + 8 > window.innerHeight ? Math.max(8, at.top - height - 8) : at.bottom + 8,
      } : {});
    }
    setOpen(!open);
    if (!card) await load();
  }
  async function follow() {
    if (!card) return;
    const result = await send(card.viewer.following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    // Following can change whether you're able to message them, so the card is fetched again.
    if (result.ok) await load();
  }
  const attributes = Object.fromEntries(Object.entries(data ?? {}).filter(([, v]) => v !== undefined).map(([k, v]) => [`data-${k}`, v]));
  return <span className="chip" ref={root}>
    <button type="button" className={className ?? "chip-button"} title={title} aria-expanded={open} aria-haspopup="dialog" onClick={toggle} {...attributes}>{children}</button>
    {open && <span className="user-card panel frame" style={place} role="dialog" aria-label={`@${username}`}>
      {error ? <span className="muted">{error}</span> : !card ? <span className="muted">Loading…</span> : <>
        <span className="card-head">
          <Avatar sizes={card.avatar} name={card.display_name} size={64} />
          <span className="grow">
            <strong className="faction-name" data-faction={card.faction ?? undefined}>{card.display_name}</strong>
            <span className="handle">@{card.username}</span>
            {card.faction && <span className="card-faction"><Crest faction={card.faction} size={16} /> {card.title}</span>}
          </span>
          {card.live && <span className="live badge">Live</span>}
        </span>
        {card.level ? <span className="card-level"><span className="player-level" data-frame={card.frame ?? 0}>Level {card.level}</span>{!card.faction && card.title && ` ${card.title}`}</span> : null}
        {card.bio && <span className="card-bio">{card.bio}</span>}
        <span className="small-text">{card.follower_count.toLocaleString()} followers · Joined {joined(card.joined_at)}</span>
        {!!card.showcase?.length && <span className="card-showcase">{card.showcase.map(x => <span key={x.key} className="chip">{x.name}</span>)}</span>}
        {!!card.also_known_as?.length && <span className="card-aka"><span className="muted">Also known as</span> {card.also_known_as.map((a, i) => <span key={a.platform}>{i > 0 && " · "}{platformNames[a.platform] || a.platform}: {a.url ? <a href={a.url} rel="nofollow noopener noreferrer" target="_blank">{a.handle}</a> : a.handle}</span>)}</span>}
        <span className="card-actions">
          {card.live && <Link className="button small" href={`/${card.username}/live`}>Watch live</Link>}
          <Link className="button small" href={`/${card.username}`}>View channel</Link>
          {card.viewer.can_message && <Link className="button small quiet" href={`/messages?with=${encodeURIComponent(card.username)}`}>Message</Link>}
          {card.viewer.signed_in && !card.viewer.is_self && !card.viewer.blocked && <button type="button" className="small quiet" onClick={follow}>{card.viewer.following ? "Following" : "Follow"}</button>}
        </span>
      </>}
      <button type="button" className="small quiet card-close" onClick={() => setOpen(false)}>Close</button>
    </span>}
  </span>;
}

/** A user's name and avatar; linked chips open the player card on click. */
export function UserChip({ user, size = 32 }: { user: Chip; size?: number }) {
  if (!user.linked || !user.username) return <span className="chip"><Avatar sizes={null} name={user.display_name} size={size} /><span><strong>{user.display_name}</strong></span></span>;
  return <PlayerCardButton username={user.username}>
    <Avatar sizes={user.avatar} name={user.display_name} size={size} />{user.faction && <Crest faction={user.faction} size={18} />}<span><strong className="faction-name" data-faction={user.faction}>{user.display_name}</strong> <span className="handle">@{user.username}</span>{user.live && <> <span className="live badge">Live</span></>}</span>
  </PlayerCardButton>;
}
