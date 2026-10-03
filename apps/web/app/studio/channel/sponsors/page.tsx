"use client";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Sponsor = { id?: string; active: boolean; name: string; description: string; link: string; discount_code: string; category: string; logo?: string | null };
const label = (v: string) => v.toLowerCase().replaceAll("_", " ").replace(/^./, c => c.toUpperCase());

export default function SponsorsStudio() {
  const [items, setItems] = useState<Sponsor[] | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [revision, setRevision] = useState<number>();
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<{ items: Sponsor[]; categories: string[]; revision: number }>("GET", "/api/me/sponsors"); if (r.ok) { setItems(r.data.items); setCategories(r.data.categories); setRevision(r.data.revision); } }, []);
  useLoad(load);
  if (!items) return <p className="loading">Loading…</p>;
  const set = (i: number, p: Partial<Sponsor>) => setItems(items.map((s, j) => j === i ? { ...s, ...p } : s));
  async function save() {
    const result = await send("PUT", "/api/me/sponsors", { items: items!.map(({ logo: _logo, ...s }) => s), revision });
    setState(result.ok ? { saved: "Sponsors saved." } : result);
    if (result.ok) load();
  }
  async function logo(id: string, file: File | undefined) {
    if (!file) return;
    const form = new FormData(); form.append("file", file);
    const result = await send("POST", `/api/me/sponsors/${id}/logo`, form);
    setState(result.ok ? { saved: "Logo uploaded." } : result);
    if (result.ok) load();
  }
  return <><h1>Sponsors</h1>
    <Section title="Sponsors" intro="Up to 10 sponsors, shown on your About tab in this order. Save a new sponsor before adding its logo (up to 2 MB).">
      {items.map((s, i) => <fieldset key={s.id ?? `new-${i}`} className="panel editor">
        <legend>{s.name || "New sponsor"}</legend>
        <label className="field"><span>Name</span><input value={s.name} maxLength={80} onChange={e => set(i, { name: e.target.value })} /></label>
        <label className="field"><span>Link</span><input type="url" value={s.link} placeholder="https://" onChange={e => set(i, { link: e.target.value })} /></label>
        <label className="field"><span>Description</span><textarea value={s.description} maxLength={300} rows={2} onChange={e => set(i, { description: e.target.value })} /></label>
        <div className="row"><label className="field narrow"><span>Category</span><select value={s.category} onChange={e => set(i, { category: e.target.value })}>{categories.map(c => <option key={c} value={c}>{label(c)}</option>)}</select></label>
          <label className="field narrow"><span>Discount code</span><input value={s.discount_code} maxLength={50} onChange={e => set(i, { discount_code: e.target.value })} /></label></div>
        <label className="checkbox"><input type="checkbox" checked={s.active} onChange={e => set(i, { active: e.target.checked })} /> Show on my channel</label>
        <div className="row">{s.id && <label className="button small quiet">{s.logo ? "Replace logo" : "Add logo"}<input className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" onChange={e => logo(s.id!, e.target.files?.[0])} /></label>}
          {s.id && s.logo && <button type="button" className="small quiet" onClick={async () => { await send("DELETE", `/api/me/sponsors/${s.id}/logo`); load(); }}>Remove logo</button>}
          <button type="button" className="small quiet" disabled={i === 0} onClick={() => { const n = [...items]; [n[i - 1], n[i]] = [n[i], n[i - 1]]; setItems(n); }}>Move up</button>
          <button type="button" className="small quiet" onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button></div>
      </fieldset>)}
      <div className="row">{items.length < 10 && <button type="button" className="small quiet" onClick={() => setItems([...items, { active: true, name: "", description: "", link: "", discount_code: "", category: categories[0] || "OTHER" }])}>Add sponsor</button>}<button type="button" className="small" onClick={save}>Save sponsors</button></div>
      <Status state={state} />
    </Section></>;
}
