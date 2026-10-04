"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { EmoteImage, type ChannelEmote } from "../../../components/Emote";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { StrikeFields, strikeFrom } from "../reports/queue";

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
  return <><h1>Media review</h1><p>Published emotes awaiting review, oldest first. Reviewing an emote also resolves its open reports.</p><p><Link href="/admin/reports">View reports</Link></p>{error && <p role="alert">{error}</p>}{items === null ? <p>Loading…</p> : items.length === 0 ? <p className="panel section muted">No emotes waiting on this page.</p> : items.map(item => <Review key={item.id} item={item} onDone={load} />)}<div className="row">{cursor && <button className="quiet small" onClick={() => setCursor("")}>Back to first page</button>}{next && <button className="quiet small" onClick={() => setCursor(next)}>Next page</button>}</div></>;
}
