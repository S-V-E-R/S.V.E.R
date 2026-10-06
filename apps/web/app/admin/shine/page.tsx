"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import type { Chip } from "../../../lib/types";

type Item = { id: string; charity: string; url: string; started_at: string; raised_cents: number; proof_url: string; status: "submitted" | "verified" | "rejected" | "revoked"; submitted_at: string; reviewed_at: string | null; review_note: string | null; owner: Chip };
const dollars = (cents: number) => `$${(cents / 100).toLocaleString(undefined, { minimumFractionDigits: 2 })}`;

/** Staff → Good Works: verify charity amounts into badges, reject them, or revoke a badge (docs/SUPPORT.md "Shine"). */
export default function AdminShine() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ items: Item[] }>("GET", "/api/admin/shine");
    if (r.ok) setItems(r.data.items); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function review(item: Item, action: "verify" | "reject" | "revoke") {
    const note = action === "verify" ? "" : window.prompt(action === "reject" ? "Why can't it be verified? The streamer sees this." : "Why is the badge revoked? The streamer sees this.")?.trim();
    if (note === undefined || (action !== "verify" && !note)) return;
    const r = await send("POST", `/api/admin/shine/${encodeURIComponent(item.id)}`, { action, note });
    setMessage(r.ok ? `${action === "verify" ? "Verified" : action === "reject" ? "Rejected" : "Revoked"}: ${item.charity}.` : r.error);
    await load();
  }
  if (!items) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  const waiting = items.filter(i => i.status === "submitted");
  const decided = items.filter(i => i.status !== "submitted");
  const row = (item: Item) => <li key={item.id}>
    <p><Link href={`/${item.owner.username}`}><strong>{item.owner.display_name}</strong></Link> · <a href={item.url} target="_blank" rel="noopener noreferrer">{item.charity}</a> · {dollars(item.raised_cents)} · stream {new Date(item.started_at).toLocaleDateString()}</p>
    <p className="small">Proof: <a href={item.proof_url} target="_blank" rel="noopener noreferrer">{item.proof_url}</a></p>
    {item.review_note && <p className="muted small">Note: {item.review_note}</p>}
    <div className="row">{item.status === "submitted" ? <><button type="button" className="small" onClick={() => review(item, "verify")}>Verify</button><button type="button" className="small quiet" onClick={() => review(item, "reject")}>Reject</button></>
      : item.status === "verified" ? <button type="button" className="small quiet danger-text" onClick={() => review(item, "revoke")}>Revoke badge</button> : <span className="muted">{item.status}</span>}</div>
  </li>;
  return <><h1>Good Works</h1>
    <p>Check that the charity is real and registered, that the link is the charity&apos;s own page or a recognized fundraiser for it, and that the proof shows the amount. Every decision is audited.</p>
    <Section title={`Waiting · ${waiting.length}`}>{waiting.length === 0 ? <p className="muted">Nothing to review.</p> : <ul className="list">{waiting.map(row)}</ul>}</Section>
    <Section title="Recent decisions">{decided.length === 0 ? <p className="muted">None in the last 90 days.</p> : <ul className="list">{decided.map(row)}</ul>}</Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
