/**
 * Platform supporter and Founder marks (docs/SUPPORTERS.md). The Wolf is S.V.E.R itself
 * (docs/LORE.md), so these use the Wolf's silver (`--wolf`), which no faction owns. They are
 * cosmetic only and are decided by the server; nothing here changes what anyone can do or see.
 */

export type SupporterTier = "wolfsworn" | "greycloak";
/** What the server says about a person (null tier when not subscribed now; Founder kept for good). */
export type SupporterMarks = { tier: SupporterTier | null; months?: number | null; since?: string | null; founder?: number | null };

const TIER_NAMES: Record<SupporterTier, string> = { wolfsworn: "Wolfsworn", greycloak: "Greycloak" };

/** The Wolf mark: an original, flat, three-tone head. `watching` adds the Greycloak eye glint. */
export function WolfMark({ size = 16, watching = false }: { size?: number; watching?: boolean }) {
  return <svg className={watching ? "wolf-mark watching" : "wolf-mark"} width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <path fill="var(--wolf)" d="M4.2 1.2 8.8 6.2h6.4l4.6-5 .8 7.4 2.4 2.6-3.1 1.2 1.7 2.8-4.4.6L12 22.8l-5.2-7-4.4-.6 1.7-2.8L1 11.2l2.4-2.6Z" />
    <path fill="var(--wolf-deep)" d="m5.3 4.2 2.4 2.8-1.9 1.6Zm13.4 0L16.3 7l1.9 1.6Z" />
    <path fill="var(--wolf-mid)" d="m12 8.4 2.5 3.6-1.1 6.6h-2.8L9.5 12Z" />
    <path fill="var(--bg)" d="m6.4 10.6 3.9 1.6-1.1 1.2Zm11.2 0-3.9 1.6 1.1 1.2Zm-7 7.8h2.8L12 20.3Z" />
    {watching && <path className="wolf-eyes" fill="var(--wolf-light)" d="m6.4 10.6 3.9 1.6-1.1 1.2Zm11.2 0-3.9 1.6 1.1 1.2Z" />}
  </svg>;
}

function months(n?: number) {
  return n && n > 0 ? `, ${n} ${n === 1 ? "month" : "months"}` : "";
}

/** Chat and user-card badge: the Wolf mark in a square tag. Greycloak's Wolf watches. */
export function SupporterBadge({ tier, monthsSupported }: { tier: SupporterTier; monthsSupported?: number }) {
  const label = `${TIER_NAMES[tier]} supporter${months(monthsSupported)}`;
  return <span className="badge supporter-badge" data-tier={tier} title={label}>
    <WolfMark size={14} watching={tier === "greycloak"} /><span className="sr-only">{label}</span>
  </span>;
}

/** A numbered Founder mark, #001 to #500, kept for good. */
export function FounderMark({ number }: { number: number }) {
  const n = String(number).padStart(3, "0");
  return <span className="badge founder-mark" title={`Founder #${n}: one of the first 500 Greycloaks`}>
    <WolfMark size={14} />Founder <span className="founder-number">#{n}</span>
  </span>;
}

/**
 * Drawn inside a supporter's player card (`.player-card[data-supporter]`): a small Wolf in the top
 * corner, and for Greycloak a thin silver line that walks the frame ("the Wolf on watch").
 */
export function SupporterFrame({ tier }: { tier: SupporterTier }) {
  return <>
    <span className="supporter-corner" aria-hidden="true"><WolfMark size={14} /></span>
    {tier === "greycloak" && <svg className="supporter-watch" aria-hidden="true" focusable="false">
      <rect width="100%" height="100%" pathLength="100" />
    </svg>}
  </>;
}

/** Profile line: "Supporting since March 2027". */
export function SupportingSince({ since }: { since: string }) {
  const d = new Date(since);
  return <span className="supporting-since"><WolfMark size={14} />Supporting since {d.toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" })}</span>;
}
