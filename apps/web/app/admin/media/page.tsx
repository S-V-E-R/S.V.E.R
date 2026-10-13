"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { EmoteImage, type ChannelEmote } from "../../../components/Emote";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { StrikeFields, strikeFrom } from "../reports/queue";
import { place, sizeLabel, type MyTile } from "../../../lib/wall";

type Item = ChannelEmote & { username: string; created_at: string; unavailable?: boolean };
function Review({ item, onDone }: { item: Item; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState("dismiss");
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const action = String(form.get("action"));
    setBusy(true);
    const r = await send("POST", `/api/admin/reports/emote/${encodeURIComponent(item.id)}/actions`, { action, note: form.get("note"), strike: action === "remove_content" ? strikeFrom(form) : undefined });
    setBusy(false); setState(r.ok ? { saved: "Review saved." } : r);
    if (r.ok) onDone();
  }
  if (item.unavailable) return <Section title={item.code}><p>This image is held for a Take It Down request.</p><Link href="/admin/take-it-down">Review Take It Down requests</Link></Section>;
  return <Section title={item.code}><div className="row wrap"><EmoteImage emote={item} size={112} /><Link href={`/admin/users/${item.username}`}>@{item.username}</Link><span className="muted">{new Date(item.created_at).toLocaleString()}</span></div>
    <form onSubmit={submit}><label className="field narrow"><span>Action</span><select name="action" value={action} onChange={e => setAction(e.target.value)}><option value="dismiss">Keep emote</option><option value="remove_content">Remove emote</option></select></label>{action === "remove_content" && <StrikeFields />}<label className="field"><span>Moderator note (required)</span><textarea name="note" required maxLength={500} rows={2} /></label><button className="small" disabled={busy}>Save review</button><Status state={state} /></form>
  </Section>;
}
type WallItem = MyTile & { owner: string | null };
/** Wall tiles and changes waiting for review (docs/WALL.md "Review"). */
function WallReview({ item, onDone }: { item: WallItem; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const change = item.status === "live" && item.pending;
  const shown = change ? item.pending! : item;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    const r = await send("POST", `/api/admin/wall/${item.n}/review`, { action: form.get("action"), note: form.get("note") });
    setBusy(false); setState(r.ok ? { saved: "Review saved." } : r);
    if (r.ok) onDone();
  }
  return <Section title={`Wall tile ${item.n}${change ? " · change" : ""}`}><div className="row wrap">
    {/* eslint-disable-next-line @next/next/no-img-element -- media CDN image */}
    {shown.image && <img src={shown.image} alt="" width={112} height={112} />}
    <div><p><strong>{shown.name}</strong> · {sizeLabel[item.size]} at {place(item)} · {item.owner ? <Link href={`/admin/users/${item.owner}`}>@{item.owner}</Link> : "no owner"}</p>{shown.link && <p>Link: {shown.link}</p>}{shown.message && <p>Message: {shown.message}</p>}</div></div>
    <p className="muted small">Nothing banned by the Community Guidelines, no gambling or betting, crypto tokens or investment schemes, political ads, adult products, or other people&apos;s trademarks without permission.</p>
    <form onSubmit={submit}><label className="field narrow"><span>Action</span><select name="action" defaultValue="approve"><option value="approve">Approve</option><option value="reject">Reject (buyer sees the reason)</option></select></label><label className="field"><span>Reason or note (required to reject)</span><textarea name="note" maxLength={500} rows={2} /></label><button className="small" disabled={busy}>Save review</button><Status state={state} /></form>
  </Section>;
}
function WallQueue() {
  const [items, setItems] = useState<WallItem[] | null>(null);
  const load = useCallback(async () => {
    const r = await send<{ items: WallItem[] }>("GET", "/api/admin/wall/review");
    if (r.ok) setItems(r.data.items);
  }, []);
  useLoad(load);
  if (!items?.length) return null;
  return <><h2>Wall tiles</h2>{items.map(item => <WallReview key={`${item.n}-${item.status}`} item={item} onDone={load} />)}</>;
}

export default function MediaQueue() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [cursor, setCursor] = useState("");
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ items: Item[]; next_cursor: string | null }>("GET", `/api/admin/media?cursor=${encodeURIComponent(cursor)}`);
    if (r.ok) { setItems(r.data.items); setNext(r.data.next_cursor); setError(""); } else setError(r.error);
  }, [cursor]);
  useLoad(load);
  return <><h1>Media review</h1><WallQueue /><h2>Emotes</h2><p>Published emotes awaiting review, oldest first. Reviewing an emote also resolves its open reports.</p><p><Link href="/admin/reports">View reports</Link></p>{error && <p role="alert">{error}</p>}{items === null ? <p>Loading…</p> : items.length === 0 ? <p className="panel section muted">No emotes waiting on this page.</p> : items.map(item => <Review key={item.id} item={item} onDone={load} />)}<div className="row">{cursor && <button className="quiet small" onClick={() => setCursor("")}>Back to first page</button>}{next && <button className="quiet small" onClick={() => setCursor(next)}>Next page</button>}</div></>;
}
