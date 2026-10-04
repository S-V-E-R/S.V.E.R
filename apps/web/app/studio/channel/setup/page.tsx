"use client";
import { useCallback, useState } from "react";
import { Section, STALE, Status, type SaveState } from "../../../../components/Form";
import { PartPicker, type Pick } from "../../../../components/PartPicker";
import { send, useLoad } from "../../../../lib/client-api";
import { setupLabel, kindLabel } from "../../../../lib/setup-parts";

type Item = { category: string; name: string; note: string; link: string | null; part_id?: string | null; review?: "PENDING" | "DISMISSED" | null; legacy_category?: string | null; kind?: string | null };
type Photo = { id: string; image: { "400": string; "1600": string }; alt: string; status: "VISIBLE" | "REMOVED" };
type Setup = { items: Item[]; title: string; description: string; photos: Photo[]; max_photos: number; categories: string[]; revision: number };
const MAX_ITEMS = 20;

export default function SetupStudio() {
  const [data, setData] = useState<Setup | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [state, setState] = useState<SaveState>({});
  const [photoState, setPhotoState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<Setup>("GET", "/api/me/setup");
    if (r.ok) { setData(r.data); setItems(r.data.items); setTitle(r.data.title); setDescription(r.data.description); setPhotos(r.data.photos); }
  }, []);
  useLoad(load);
  if (!data) return <p className="loading">Loading…</p>;
  const set = (i: number, p: Partial<Item>) => setItems(items.map((s, j) => j === i ? { ...s, ...p } : s));
  // Items stay grouped by category (picker order, then "Other"); owner order within a category.
  const order = [...data.categories, "OTHER"];
  const sorted = (list: Item[]) => [...list].sort((a, b) => order.indexOf(a.category) - order.indexOf(b.category));
  const add = (category: string, pick: Pick) => setItems(sorted([...items, { category, name: pick.name, note: "", link: null, part_id: pick.part_id, review: null, kind: pick.kind ?? null }]));
  const moveItem = (i: number, d: number) => { const next = [...items]; [next[i], next[i + d]] = [next[i + d], next[i]]; setItems(next); };
  async function save() {
    const result = await send("PUT", "/api/me/setup", { items: sorted(items).map(i => ({ category: i.category, name: i.name, note: i.note, link: i.link?.trim() ? i.link : null, ...(i.part_id ? { part_id: i.part_id } : {}) })), title, description, revision: data!.revision });
    setState(result.ok ? { saved: "Setup saved." } : result.status === 409 ? { error: STALE } : result);
    if (result.ok) load();
  }
  async function upload(file: File | undefined, input: HTMLInputElement) {
    if (!file) return;
    const form = new FormData();
    form.append("file", file);
    setPhotoState({ saved: "Uploading…" });
    const r = await send<Photo>("POST", "/api/me/setup/photos", form);
    input.value = "";
    setPhotoState(r.ok ? { saved: "Photo added." } : r);
    if (r.ok) setPhotos([...photos, r.data]);
  }
  async function savePhotos(next: Photo[]) {
    const r = await send("PUT", "/api/me/setup/photos", { photos: next.map(p => ({ id: p.id, alt: p.alt })) });
    setPhotoState(r.ok ? { saved: "Photos saved." } : r);
    if (r.ok) setPhotos(next); else load();
  }
  async function remove(id: string) {
    const r = await send("DELETE", `/api/me/setup/photos/${encodeURIComponent(id)}`);
    setPhotoState(r.ok ? { saved: "Photo deleted." } : r);
    if (r.ok) setPhotos(photos.filter(p => p.id !== id));
  }
  const move = (i: number, d: number) => { const next = [...photos]; [next[i], next[i + d]] = [next[i + d], next[i]]; return next; };
  const err = (f: string) => state.field === f ? state.error : undefined;
  return <><h1>Streaming setup</h1>
    <Section title="About your setup" intro="Shown above your gear on the About tab. Plain text.">
      <label className="field"><span>Title</span><input value={title} maxLength={80} onChange={e => setTitle(e.target.value)} aria-invalid={!!err("title")} placeholder="My battle station" /><small>Up to 80 characters.</small></label>
      <label className="field"><span>Description</span><textarea value={description} maxLength={500} rows={4} onChange={e => setDescription(e.target.value)} aria-invalid={!!err("description")} /><small>Up to 500 characters and 6 line breaks.</small></label>
    </Section>
    <Section title={`Photos (${photos.length} of ${data.max_photos})`} intro="Up to 3 photos of your setup. JPG, PNG or WebP up to 5 MB, at least 64 px. Photos show right away; image metadata is removed.">
      {photos.length > 0 && <ul className="setup-photo-editor">{photos.map((p, i) => <li key={p.id} className="panel">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={p.image["400"]} alt={p.alt || `Setup photo ${i + 1}`} width={160} />
        {p.status === "REMOVED" && <span className="badge">Removed by moderators</span>}
        <label className="field"><span>Description for screen readers</span><input value={p.alt} maxLength={120} onChange={e => setPhotos(photos.map(x => x.id === p.id ? { ...x, alt: e.target.value } : x))} /></label>
        <div className="row tight">
          {i > 0 && <button type="button" className="small quiet" onClick={() => savePhotos(move(i, -1))}>Move left</button>}
          {i < photos.length - 1 && <button type="button" className="small quiet" onClick={() => savePhotos(move(i, 1))}>Move right</button>}
          <button type="button" className="small quiet" onClick={() => remove(p.id)}>Delete</button>
        </div>
      </li>)}</ul>}
      <div className="row">
        {photos.length < data.max_photos && <label className="button small quiet">Add photo<input className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" onChange={e => upload(e.target.files?.[0], e.target)} /></label>}
        {photos.length > 0 && <button type="button" className="small quiet" onClick={() => savePhotos(photos)}>Save photo descriptions</button>}
      </div>
      <Status state={photoState} />
    </Section>
    <Section title={`Your gear (${items.length} of ${MAX_ITEMS})`} intro="Search SVER's parts list in each category. If yours isn't listed, add it as a custom entry: it shows on your page right away, and staff may add it to the list. Nothing is saved until you press Save setup.">
      {[...data.categories, ...(items.some(i => i.category === "OTHER") ? ["OTHER"] : [])].map(category => {
        const rows = items.map((s, i) => [s, i] as const).filter(([s]) => s.category === category);
        return <div key={category} className="gear-category" data-category={category}>
          <h3>{setupLabel(category)}</h3>
          {category === "OTHER" && <p className="muted">Kept from before the parts list. You can change the note and link or remove these; new items go in one of the categories above.</p>}
          {rows.length > 0 && <ul className="gear-list">{rows.map(([s, i], k) => <li key={i} className="gear-item">
            <div className="row wrap">
              {s.part_id ? <span className="gear-name"><strong>{s.name}</strong> <span className="tag">SVER list</span>{kindLabel(s.kind) && <> <span className="tag">{kindLabel(s.kind)}</span></>}</span>
                : category === "OTHER" ? <span className="gear-name"><strong>{s.name}</strong></span>
                : <span className="gear-name"><input aria-label={`${setupLabel(category)} name`} value={s.name} maxLength={80} onChange={e => set(i, { name: e.target.value, review: null })} />
                  {kindLabel(s.kind) && <span className="tag">{kindLabel(s.kind)}</span>}
                  {category !== "OTHER" && <span className="tag">{s.review === "PENDING" ? "Custom · waiting for review" : "Custom"}</span>}</span>}
              <input aria-label={`Note for ${s.name || "this item"}`} value={s.note} maxLength={120} placeholder="Note (optional)" onChange={e => set(i, { note: e.target.value })} />
              <input aria-label={`Link for ${s.name || "this item"}`} type="url" value={s.link || ""} placeholder="https:// (optional)" onChange={e => set(i, { link: e.target.value })} />
              <div className="row tight">
                {k > 0 && <button type="button" className="small quiet" aria-label={`Move ${s.name} up`} onClick={() => moveItem(i, rows[k - 1][1] - i)}>Up</button>}
                {k < rows.length - 1 && <button type="button" className="small quiet" aria-label={`Move ${s.name} down`} onClick={() => moveItem(i, rows[k + 1][1] - i)}>Down</button>}
                <button type="button" className="small quiet" aria-label={`Remove ${s.name}`} onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button>
              </div>
            </div>
          </li>)}</ul>}
          {category !== "OTHER" && <PartPicker category={category} label={setupLabel(category)} disabled={items.length >= MAX_ITEMS} onPick={pick => add(category, pick)} />}
        </div>;
      })}
      {items.length >= MAX_ITEMS && <p className="muted">You&apos;ve reached 20 items. Remove one to add another.</p>}
      <div className="row"><button type="button" className="small" onClick={save}>Save setup</button></div>
      <Status state={state} />
    </Section></>;
}
