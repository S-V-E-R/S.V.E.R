/** The Wall (docs/WALL.md): 1,000 cells sold once as seats, tables and halls. */
export type WallSize = "seat" | "table" | "hall";
export type WallTile = {
  n: number; x: number; y: number; size: WallSize; side: number;
  state: "live" | "claiming" | "review" | "removed";
  name?: string; link?: string | null; message?: string | null; crest?: string | null;
  image?: string | null; image_large?: string | null; owner?: string | null; approved_at?: string | null;
};
export type MyTile = {
  n: number; x: number; y: number; size: WallSize; side: number;
  status: "held" | "expired" | "in_review" | "live" | "rejected" | "refund_due" | "refunded" | "removed";
  name: string | null; link: string | null; message: string | null; crest: string | null; image: string | null;
  min_picture: number; pending: { name: string; link: string | null; message: string | null; image: string | null } | null;
  review_note: string; price_cents: number | null; hold_until: string; paid_at: string | null;
  rejected_at: string | null; approved_at: string | null; resubmit_until: string | null; refundable: boolean;
  hold_active: boolean; can_edit: boolean;
};
export type WallPrice = { size: WallSize; side: number; min_picture: number; cents: number | null; limit: number | null; taken: number | null };
export type WallData = {
  columns: number; rows: number; tiles: WallTile[]; taken: number; sold: number; full: boolean;
  stage: number; next_stage_at: number | null; prices: WallPrice[];
  sales_open: boolean; can_buy: boolean; signed_in: boolean; mine: MyTile[]; hold_minutes: number;
};

export const sizeLabel: Record<WallSize, string> = { seat: "Seat", table: "Table", hall: "Hall" };
export const dollars = (cents: number | null | undefined) => cents == null ? "—" : `$${(cents / 100).toLocaleString("en-US", { minimumFractionDigits: cents % 100 ? 2 : 0 })}`;
/** Column and row as people count them (from 1). */
export const place = (t: { x: number; y: number }) => `column ${t.x + 1}, row ${t.y + 1}`;
export const stageName = (stage: number) => stage === 0 ? "First light" : `Stage ${stage + 1}`;
export const statusLabel: Record<MyTile["status"], string> = {
  held: "Held for you", expired: "Hold ended", in_review: "Taken · in review", live: "Live", rejected: "Not approved",
  refund_due: "Refund on its way", refunded: "Refunded", removed: "Removed",
};
/** Link attributes for a buyer's link (docs/WALL.md "Links"). */
export const tileLinkRel = "sponsored nofollow ugc noopener";
