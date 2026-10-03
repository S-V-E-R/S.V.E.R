"use client";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Item = { category: string; name: string; note: string; link: string | null };
const label = (v: string) => v.toLowerCase().replaceAll("_", " ").replace(/^./, c => c.toUpperCase());

export default function SetupStudio() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [revision, setRevision] = useState<number>();
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<{ items: Item[]; categories: string[]; revision: number }>("GET", "/api/me/setup"); if (r.ok) { setItems(r.data.items); setCategories(r.data.categories); setRevision(r.data.revision); } }, []);
  useLoad(load);
  if (!items) return <p className="loading">Loading…</p>;
  const set = (i: number, p: Partial<Item>) => setItems(items.map((s, j) => j === i ? { ...s, ...p } : s));
  async function save() {
    const result = await send("PUT", "/api/me/setup", { items: items!.map(i => ({ ...i, link: i.link?.trim() ? i.link : null })), revision });
    setState(result.ok ? { saved: "Setup saved." } : result);
    if (result.ok) load();
  }
  return <><h1>Streaming setup</h1>
    <Section title="Your gear" intro="Up to 20 items, shown on your About tab.">
      {items.map((s, i) => <div key={i} className="row wrap">
        <select aria-label="Category" value={s.category} onChange={e => set(i, { category: e.target.value })}>{categories.map(c => <option key={c} value={c}>{label(c)}</option>)}</select>
        <input aria-label="Name" value={s.name} maxLength={80} placeholder="Name" onChange={e => set(i, { name: e.target.value })} />
        <input aria-label="Note" value={s.note} maxLength={120} placeholder="Note (optional)" onChange={e => set(i, { note: e.target.value })} />
        <input aria-label="Link" type="url" value={s.link || ""} placeholder="https:// (optional)" onChange={e => set(i, { link: e.target.value })} />
        <button type="button" className="small quiet" onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button>
      </div>)}
      <div className="row">{items.length < 20 && <button type="button" className="small quiet" onClick={() => setItems([...items, { category: categories[0] || "OTHER", name: "", note: "", link: null }])}>Add item</button>}<button type="button" className="small" onClick={save}>Save setup</button></div>
      <Status state={state} />
    </Section></>;
}
