"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { send } from "../lib/client-api";
import { dollars, statusLabel, type MyTile } from "../lib/wall";
import { Status, type SaveState } from "./Form";
import { TileForm } from "./WallSeats";

const day = (iso: string) => new Date(iso).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" });

/** The owner's panel on /wall/{n}: status, review notes, changes and refunds (docs/WALL.md). */
export function TileOwner({ tile }: { tile: MyTile }) {
  const router = useRouter();
  const [state, setState] = useState<SaveState>({});
  const [confirm, setConfirm] = useState(false);
  async function refund() {
    const r = await send("POST", `/api/wall/${tile.n}/refund`);
    setState(r.ok ? { saved: "Refund requested. Your spot is free again, and the money returns to your card within a few days." } : r);
    if (r.ok) router.refresh();
  }
  return <section className="panel section" aria-labelledby="owner-title">
    <h2 id="owner-title">Your tile · {statusLabel[tile.status]}</h2>
    {tile.price_cents != null && tile.paid_at && <p className="muted">Paid {dollars(tile.price_cents)} on {day(tile.paid_at)}.</p>}
    {tile.status === "held" && <p>This spot is held for you until {new Date(tile.hold_until).toLocaleTimeString()}. Finish paying from <Link href="/wall">the Wall</Link>.</p>}
    {tile.status === "in_review" && <p>Staff review every tile before it shows. It appears on the Wall as &ldquo;taken · in review&rdquo; until then.</p>}
    {tile.status === "rejected" && <div role="alert"><p><strong>Not approved:</strong> {tile.review_note}</p>{tile.resubmit_until && <p>You can change it and resubmit until {day(tile.resubmit_until)}, or ask for a full refund. After that date the refund is automatic.</p>}</div>}
    {tile.status === "live" && tile.pending && <p>A change is waiting for review. Your tile stays as it is until it&apos;s approved.</p>}
    {tile.status === "live" && !tile.pending && tile.review_note && <p><strong>Your last change wasn&apos;t approved:</strong> {tile.review_note}</p>}
    {tile.status === "removed" && <p>This tile was removed for breaking the Wall terms. Its spot stays empty.</p>}
    {tile.can_edit && <details open={tile.status === "rejected"}><summary>{tile.status === "live" ? "Change your tile" : "Change and resubmit"}</summary><TileForm tile={tile} mode="edit" /></details>}
    {tile.refundable && (confirm
      ? <div className="row wrap"><span>Refund {dollars(tile.price_cents)} and give the spot back?</span><button type="button" className="small danger" onClick={refund}>Yes, refund</button><button type="button" className="small quiet" onClick={() => setConfirm(false)}>Keep it</button></div>
      : <button type="button" className="small quiet" onClick={() => setConfirm(true)}>Ask for a full refund</button>)}
    <Status state={state} />
  </section>;
}
