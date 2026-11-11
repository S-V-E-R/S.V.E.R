"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { ListingBody, type ListingContent } from "../../../components/Catalog";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Item = { client_id: string; slug: string; status: string; app: string; owner: string | null; suspended: boolean; submitted_at: string; pending?: ListingContent; live?: ListingContent };

/** One listing: what's waiting (if anything), what's public, and the review form. */
function Review({ item, onDone }: { item: Item; onDone: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const shown = item.pending ?? item.live;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    const r = await send("POST", `/api/admin/catalog/${encodeURIComponent(item.client_id)}/review`, { action: form.get("action"), note: form.get("note") });
    setBusy(false); setState(r.ok ? { saved: "Review saved." } : r);
    if (r.ok) onDone();
  }
  if (!shown) return null;
  return <Section title={`${shown.name}${item.pending ? (item.live ? " · change" : " · new") : ""}`}>
    <p>App “{item.app}” · {item.owner ? <Link href={`/admin/users/${item.owner}`}>@{item.owner}</Link> : "no owner"} · sent {new Date(item.submitted_at).toLocaleString()}{item.suspended && " · app suspended (hidden)"}{item.live && <> · <Link href={`/crowdsync/${item.slug}`}>public page</Link></>}</p>
    {item.pending && item.live && <p className="muted small">Public now: “{item.live.name}”: {item.live.summary}</p>}
    <p><strong>{shown.summary}</strong></p>
    <ListingBody listing={shown} />
    <p className="muted small">Nothing banned by the Community Guidelines, no gambling or betting, no paid advantage over other viewers, no other people&apos;s names or trademarks without permission, and the link must lead to the game or its maker.</p>
    <form onSubmit={submit}><label className="field narrow"><span>Action</span><select name="action" defaultValue={item.pending ? "approve" : "remove"}>
      {item.pending && <><option value="approve">Approve</option><option value="reject">Reject (the developer sees the reason)</option></>}
      <option value="remove">Remove from the catalog (the developer sees the reason)</option>
    </select></label><label className="field"><span>Reason or note (required to reject or remove)</span><textarea name="note" maxLength={500} rows={2} /></label><button className="small" disabled={busy}>Save review</button><Status state={state} /></form>
  </Section>;
}

/** /admin/catalog: CrowdSync catalog listings waiting for review, then everything public (docs/CROWDSYNC.md "Catalog"). */
export default function CatalogQueue() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ items: Item[] }>("GET", "/api/admin/catalog");
    if (r.ok) { setItems(r.data.items); setError(""); } else setError(r.error);
  }, []);
  useLoad(load);
  const waiting = items?.filter(i => i.pending) ?? [], live = items?.filter(i => !i.pending) ?? [];
  return <><h1>CrowdSync catalog</h1>
    <p>New listings and changes wait here, oldest first. An approved listing stays public as it was while a change waits. Every action is in the <Link href="/admin/audit">audit log</Link>.</p>
    {error && <p role="alert">{error}</p>}
    {items === null ? <p>Loading…</p> : <>
      <h2>Waiting for review</h2>
      {waiting.length === 0 ? <p className="panel section muted">Nothing is waiting.</p> : waiting.map(item => <Review key={`${item.client_id}-${item.submitted_at}`} item={item} onDone={load} />)}
      <h2>In the catalog</h2>
      {live.length === 0 ? <p className="panel section muted">Nothing is listed.</p> : live.map(item => <details key={item.client_id}><summary>{item.live?.name} · {item.owner ? `@${item.owner}` : "no owner"}{item.suspended && " · app suspended (hidden)"}</summary><Review item={item} onDone={load} /></details>)}
    </>}
  </>;
}
