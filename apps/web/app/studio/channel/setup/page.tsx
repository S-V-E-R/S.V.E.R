"use client";
import { useCallback, useState } from "react";
import { Section, STALE, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Item = { category: string; name: string; note: string; link: string | null };
type Photo = { id: string; image: { "400": string; "1600": string }; alt: string; status: "VISIBLE" | "REMOVED" };
type Setup = { items: Item[]; title: string; description: string; photos: Photo[]; max_photos: number; categories: string[]; revision: number };
const label = (v: string) => v.toLowerCase().replaceAll("_", " ").replace(/^./, c => c.toUpperCase());

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
  async function save() {
    const result = await send("PUT", "/api/me/setup", { items: items.map(i => ({ ...i, link: i.link?.trim() ? i.link : null })), title, description, revision: data!.revision });
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
    <Section title="Your gear" intro="Up to 20 items, shown on your About tab.">
      {items.map((s, i) => <div key={i} className="row wrap">
        <select aria-label="Category" value={s.category} onChange={e => set(i, { category: e.target.value })}>{data.categories.map(c => <option key={c} value={c}>{label(c)}</option>)}</select>
        <input aria-label="Name" value={s.name} maxLength={80} placeholder="Name" onChange={e => set(i, { name: e.target.value })} />
        <input aria-label="Note" value={s.note} maxLength={120} placeholder="Note (optional)" onChange={e => set(i, { note: e.target.value })} />
        <input aria-label="Link" type="url" value={s.link || ""} placeholder="https:// (optional)" onChange={e => set(i, { link: e.target.value })} />
        <button type="button" className="small quiet" onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button>
      </div>)}
      <div className="row">{items.length < 20 && <button type="button" className="small quiet" onClick={() => setItems([...items, { category: data.categories[0] || "OTHER", name: "", note: "", link: null }])}>Add item</button>}<button type="button" className="small" onClick={save}>Save setup</button></div>
      <Status state={state} />
    </Section></>;
}
