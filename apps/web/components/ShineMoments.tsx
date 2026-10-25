"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "./Form";
import { send, useLoad } from "../lib/client-api";

type Moment = { video_id: string; title: string; description: string };

/** Creator Studio → Shine → Moments: up to 10 of your own public clips and Highlights, in order. */
export function ShineMoments() {
  const [me, setMe] = useState("");
  const [items, setItems] = useState<Moment[]>([]);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const who = await send<{ username: string }>("GET", "/api/auth/me");
    if (!who.ok) { setMessage(who.error); return; }
    setMe(who.data.username);
    const r = await send<{ moments: Moment[] }>("GET", `/api/channels/${encodeURIComponent(who.data.username)}/moments`);
    if (r.ok) setItems(r.data.moments);
  }, []);
  useLoad(load);
  const idOf = (link: string) => link.trim().match(/(?:clips|videos)\/([0-9a-f-]{36})/i)?.[1] ?? link.trim();
  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const id = idOf(String(form.get("link") ?? ""));
    if (items.some(m => m.video_id === id)) { setMessage("That one is already pinned."); return; }
    setItems(list => [...list, { video_id: id, title: String(form.get("title") ?? ""), description: "" }]);
    setMessage("Not saved yet: press Save to update your channel page.");
    event.currentTarget.reset();
  }
  const move = (i: number, by: number) => setItems(list => { setMessage("Not saved yet: press Save to update your channel page."); const next = [...list]; const [m] = next.splice(i, 1); next.splice(Math.max(0, Math.min(next.length, i + by)), 0, m); return next; });
  async function save() {
    const r = await send<{ moments: Moment[] }>("PUT", "/api/me/moments", { items });
    if (r.ok) { setItems(r.data.moments); setMessage("Saved."); } else setMessage(r.error);
  }
  return <div id="moments"><Section title="Shine Moments" intro="Up to 10 of your own public clips and Highlights. A clip made private or removed drops off on its own.">
      {message && <p role="status" className="form-message">{message}</p>}
      <ol className="list">{items.map((m, i) => <li key={m.video_id} className="row wrap"><span>{m.title || m.video_id}</span>
        <button type="button" className="small quiet" onClick={() => move(i, -1)} disabled={i === 0}>Up</button>
        <button type="button" className="small quiet" onClick={() => move(i, 1)} disabled={i === items.length - 1}>Down</button>
        <button type="button" className="small quiet danger-text" onClick={() => { setItems(list => list.filter((_, j) => j !== i)); setMessage("Not saved yet: press Save to update your channel page."); }}>Remove</button></li>)}</ol>
      {items.length < 10 && <form onSubmit={add} className="row wrap">
        <label className="field"><span>Clip or Highlight link</span><input name="link" required placeholder={`https://sver.tv/clips/…`} /></label>
        <label className="field"><span>Title (optional)</span><input name="title" maxLength={100} /></label>
        <button type="submit" className="small">Add</button>
      </form>}
      <button type="button" onClick={save} disabled={!me}>Save</button>
    </Section></div>;
}
