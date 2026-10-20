"use client";
/* eslint-disable @next/next/no-img-element -- already-sized, immutable media bucket variants */
import { useRef, useState } from "react";
import { Crest } from "./Crest";
import type { ChannelEmote } from "./Emote";
import { factionOf } from "../lib/factions";

const CARD_WIDTH = 220;

/**
 * Another channel's signature emote in chat: hovering or focusing it shows the owner's crest and
 * name (docs/CHANNEL_ADDITIONS.md "Signature emote"). The card is fixed to the viewport, so the
 * chat's scrolling list can't clip it; it sits above the emote, or below it near the top.
 */
export function SignatureEmote({ emote, owner }: { emote: ChannelEmote; owner: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [at, setAt] = useState<{ left: number; top: number; below: boolean } | null>(null);
  const faction = factionOf(emote.faction ?? null);
  const label = `${emote.code}, @${owner}'s signature emote${faction ? ` (${faction.name})` : ""}`;
  const show = () => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    const below = r.top < 64;
    setAt({ left: Math.max(8, Math.min(r.left, window.innerWidth - CARD_WIDTH - 8)), top: below ? r.bottom + 6 : r.top - 6, below });
  };
  return <span ref={ref} className="signature-emote" tabIndex={0} aria-label={label} onMouseEnter={show} onMouseLeave={() => setAt(null)} onFocus={show} onBlur={() => setAt(null)}>
    <img className="chat-emote" src={emote.image[28]} srcSet={`${emote.image["56"]} 2x, ${emote.image["112"]} 4x`} width={28} height={28} alt="" loading="lazy" />
    {at && <span className="signature-card" data-below={at.below || undefined} style={{ left: at.left, top: at.top, maxWidth: CARD_WIDTH }} aria-hidden="true">
      <Crest faction={emote.faction ?? null} initial={owner.slice(0, 1).toUpperCase()} size={24} />
      <span><strong>{emote.code.split("/")[1]}</strong><span className="muted">@{owner}&apos;s signature</span></span>
    </span>}
  </span>;
}
