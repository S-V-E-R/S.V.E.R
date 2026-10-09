"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { EmoteImage, type ChannelEmote } from "../../../components/Emote";
import { send, useLoad } from "../../../lib/client-api";

type Item = ChannelEmote & { status: "VISIBLE" | "REMOVED" | "UNAVAILABLE"; signature?: boolean; reviewed?: boolean };
export default function EmotesStudio() {
  const [items, setItems] = useState<Item[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const [tier, setTier] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ items: Item[] }>("GET", "/api/me/emotes");
    if (r.ok) { setItems(r.data.items); setLoaded(true); } else setState(r);
  }, []);
  useLoad(load);
  async function upload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    setBusy(true);
    const r = await send("POST", "/api/me/emotes", new FormData(form));
    setBusy(false); setState(r.ok ? { saved: "Emote added." } : r);
    if (r.ok) { form.reset(); await load(); }
  }
  async function remove(item: Item) {
    if (!window.confirm(`Delete ${item.code}? Future chat rendering will show its code as text.`)) return;
    setBusy(true);
    const r = await send("DELETE", `/api/me/emotes/${encodeURIComponent(item.id)}`);
    setBusy(false); setState(r.ok ? { saved: "Emote deleted." } : r);
    if (r.ok) await load();
  }
  async function signature(item: Item) {
    const r = await send("PUT", `/api/me/emotes/${encodeURIComponent(item.id)}/signature`, { on: !item.signature });
    if (r.ok) await load(); else setState(r);
  }
  const open = items.filter(i => !i.tier).length;
  const full = tier ? items.filter(i => i.tier === Number(tier)).length >= 5 : open >= 10;
  return <><h1>Channel emotes</h1><p>Open emotes are for everyone in your chat; subscriber emotes are for subscribers of that tier or higher. Codes are case-sensitive and must be typed as a whole word, separated by spaces.</p>
    <Section title={`Emotes · ${open}/10 open · ${[1, 2, 3].map(t => `Tier ${t} ${items.filter(i => i.tier === t).length}/5`).join(" · ")}`}>
      {!loaded ? <p>Loading…</p> : items.length === 0 ? <p className="muted">No emotes yet.</p> : <ul className="list">{items.map(item => <li className="row between" key={item.id}><span className="row">{item.status === "VISIBLE" && <EmoteImage emote={item} />}<strong>{item.code}</strong>{item.tier && <span className="badge sub-badge">Tier {item.tier}</span>}{item.status !== "VISIBLE" && <span className="muted">{item.status === "REMOVED" ? "Removed by staff" : "Unavailable"}</span>}</span><span className="row">{!item.tier && item.status === "VISIBLE" && <button type="button" className="quiet small" disabled={busy} onClick={() => void signature(item)}>{item.signature ? (item.reviewed ? "Signature · works everywhere" : "Signature · awaiting staff review") : "Make signature"}</button>}<button className="quiet small danger-text" disabled={busy} onClick={() => remove(item)}>Delete<span className="sr-only"> {item.code}</span></button></span></li>)}</ul>}
      <p className="muted small">Your signature emote (one open emote) works in every chat, written as yourname/Code, once staff approve it. Channels can turn signature emotes off in their chat.</p>
    </Section>
    <Section title="Upload emote"><form onSubmit={upload}>
      <p className="muted">Square PNG or WebP, at least 112 × 112 pixels, up to 1 MB. Animated images use their first frame. Emotes publish immediately.</p>
      <label className="field narrow"><span>Code</span><input name="code" required minLength={3} maxLength={20} pattern="[A-Za-z0-9]{3,20}" autoComplete="off" /><small>3–20 ASCII letters and digits.</small></label>
      <label className="field narrow"><span>Who can use it</span><select name="tier" value={tier} onChange={e => setTier(e.target.value)}><option value="">Everyone (open)</option><option value="1">Tier 1 subscribers</option><option value="2">Tier 2 subscribers</option><option value="3">Tier 3 subscribers</option></select></label>
      <label className="field"><span>Image</span><input type="file" name="file" required accept="image/png,image/webp" /></label>
      <button disabled={busy || !loaded || full}>Upload emote</button>
    </form></Section><Status state={state} /></>;
}
