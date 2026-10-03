"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { UserChip } from "../../../../components/UserChip";
import { send, useLoad } from "../../../../lib/client-api";
import type { Chip } from "../../../../lib/types";

type Settings = { who_can_post: string; require_approval: boolean; hold_links: boolean; hold_new_accounts: boolean };
type Mine = { settings: Settings; pins: { id: string; body: string; position: number }[]; pending_count: number; revision: number };
type Pending = { kind: "post" | "reply"; id: string; body: string; created_at: string; author: Chip; post_id: string | null };

export default function WallStudio() {
  const [mine, setMine] = useState<Mine | null>(null);
  const [pending, setPending] = useState<{ items: Pending[]; next_cursor: string | null } | null>(null);
  const [state, setState] = useState<SaveState>({});
  const [review, setReview] = useState<SaveState>({});
  const load = useCallback(async () => {
    const [m, p] = await Promise.all([send<Mine>("GET", "/api/me/wall"), send<{ items: Pending[]; next_cursor: string | null }>("GET", "/api/me/wall/pending")]);
    if (m.ok) setMine(m.data);
    if (p.ok) setPending(p.data);
  }, []);
  useLoad(load);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const result = await send("PUT", "/api/me/wall/settings", { who_can_post: f.get("who_can_post"), require_approval: f.has("require_approval"), hold_links: f.has("hold_links"), hold_new_accounts: f.has("hold_new_accounts"), revision: mine?.revision });
    setState(result.ok ? { saved: "Wall settings saved." } : result);
    if (result.ok) load();
  }
  async function act(item: Pending, action: "approve" | "reject" | "block-author") {
    if (action === "block-author" && !window.confirm("Block this author? Their pending posts are rejected and you stop following each other.")) return;
    const result = await send("POST", `/api/wall/${item.kind === "post" ? "posts" : "replies"}/${item.id}/${action}`);
    setReview(result.ok ? { saved: "Done." } : result);
    load();
  }
  async function unpin(id: string) {
    const result = await send("PUT", "/api/me/wall/pins", { post_ids: mine!.pins.filter(p => p.id !== id).map(p => p.id) });
    setState(result.ok ? { saved: "Pins saved." } : result);
    load();
  }
  if (!mine) return <p className="loading">Loading…</p>;
  const s = mine.settings;
  return <><h1>Wall</h1>
    <Section title="Who can post">
      <form onSubmit={save}>
        <label className="field narrow"><span>Who can sign your Wall</span><select name="who_can_post" defaultValue={s.who_can_post}><option value="ANYONE">Anyone with a verified email</option><option value="FOLLOWING">People you follow</option><option value="MUTUAL">Mutual follows</option><option value="NONE">Only you</option></select></label>
        <label className="checkbox"><input type="checkbox" name="require_approval" defaultChecked={s.require_approval} /> Approve every post and reply first</label>
        <label className="checkbox"><input type="checkbox" name="hold_links" defaultChecked={s.hold_links} /> Hold posts with links for approval</label>
        <label className="checkbox"><input type="checkbox" name="hold_new_accounts" defaultChecked={s.hold_new_accounts} /> Hold posts from accounts less than 7 days old</label>
        <button type="submit" className="small">Save settings</button>
        <Status state={state} />
      </form>
    </Section>
    <Section title={`Waiting for approval (${mine.pending_count})`}>
      {!pending?.items.length ? <p className="muted">Nothing waiting.</p> : <ul className="list">{pending.items.map(i => <li key={i.id}>
        <div className="row"><UserChip user={i.author} size={24} /><span className="badge">{i.kind === "post" ? "Post" : "Reply"}</span></div>
        <p className="wall-body">{i.body}</p>
        <div className="row"><button type="button" className="small" onClick={() => act(i, "approve")}>Approve</button><button type="button" className="small quiet" onClick={() => act(i, "reject")}>Reject</button><button type="button" className="small danger" onClick={() => act(i, "block-author")}>Block author</button></div>
      </li>)}</ul>}
      <Status state={review} />
    </Section>
    <Section title="Pinned posts" intro="Pin up to 3 approved posts from your channel's Wall tab. Pinned posts show first.">
      {mine.pins.length === 0 ? <p className="muted">No pinned posts.</p> : <ol className="list">{mine.pins.map(p => <li key={p.id} className="row between"><span>{p.body}</span><button type="button" className="small quiet" onClick={() => unpin(p.id)}>Unpin</button></li>)}</ol>}
    </Section></>;
}
