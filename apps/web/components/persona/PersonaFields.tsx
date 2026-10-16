"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../Form";
import { send, useLoad } from "../../lib/client-api";
import { SECTION_NAMES, type HomeSection } from "../../lib/types";

type Persona = {
  currently_verb: string; currently_text: string; song_why: string; theme_variant: string; variants: string[]; faction: string | null;
  page_layout: "" | "stream" | "persona"; home_order: HomeSection[]; verbs: { verb: string; label: string }[];
};
const VARIANT_NAMES: Record<string, string> = { "": "Default", forge: "Forge", ash: "Ash", moon: "Moon", deep: "Deep", crown: "Crown", dawn: "Dawn" };
const DEFAULT_NAMES: Record<string, string> = { myria: "Ember", aetheron: "Violet", glint: "Gold" };

/** Loads /api/me/persona once per editor; partial saves send no revision, so the editors don't trip over each other. */
function usePersona() {
  const [data, setData] = useState<Persona | null>(null);
  const load = useCallback(async () => { const r = await send<Persona>("GET", "/api/me/persona"); if (r.ok) setData(r.data); }, []);
  useLoad(load);
  async function save(change: Partial<Persona>): Promise<SaveState> {
    const r = await send("PUT", "/api/me/persona", change);
    if (r.ok) { await load(); return { saved: "Saved." }; }
    return r;
  }
  return { data, save };
}

/** "Currently": a word from a fixed list and up to 60 characters. While you're live, your page says what you're streaming instead. */
export function CurrentlyEditor({ onSaved }: { onSaved?: () => void }) {
  const { data, save } = usePersona();
  const [state, setState] = useState<SaveState>({});
  if (!data) return null;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const result = await save({ currently_verb: String(f.get("verb") ?? ""), currently_text: String(f.get("text") ?? "") });
    setState(result); if (result.saved) onSaved?.();
  }
  return <form onSubmit={submit} key={data.currently_verb + data.currently_text}>
    <Section title="Currently" intro="What you're into right now, under your name. While you're live it says what you're streaming instead.">
      <div className="row"><label className="field narrow"><span>Word</span><select name="verb" defaultValue={data.currently_verb}><option value="">None</option>{data.verbs.map(v => <option key={v.verb} value={v.verb}>{v.label}</option>)}</select></label>
        <label className="field grow"><span>What</span><input name="text" defaultValue={data.currently_text} maxLength={60} placeholder="Hollow Knight, my first Steel Soul run" aria-invalid={state.field === "currently_text"} /><small>Up to 60 characters.</small></label></div>
      <button type="submit" className="small">Save Currently</button><Status state={state} />
    </Section>
  </form>;
}

/** The short line under your profile song. */
export function SongWhyEditor({ onSaved }: { onSaved?: () => void }) {
  const { data, save } = usePersona();
  const [state, setState] = useState<SaveState>({});
  if (!data) return null;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const result = await save({ song_why: String(new FormData(event.currentTarget).get("why") ?? "") });
    setState(result); if (result.saved) onSaved?.();
  }
  return <form onSubmit={submit} className="row" key={data.song_why}>
    <label className="field grow"><span>Why this song</span><input name="why" defaultValue={data.song_why} maxLength={80} placeholder="It plays in my head every reset" /><small>Optional. Up to 80 characters, shown under the song.</small></label>
    <button type="submit" className="small">Save</button><Status state={state} />
  </form>;
}

/** One of three variants of your faction's colors, for your own page only. */
export function ThemeEditor({ onSaved }: { onSaved?: () => void }) {
  const { data, save } = usePersona();
  const [state, setState] = useState<SaveState>({});
  if (!data) return null;
  if (!data.faction) return <Section title="Page colors"><p className="muted">Your page uses neutral steel until you pick a side. Each faction has three color variants to choose from.</p></Section>;
  const options = ["", ...data.variants];
  async function pick(variant: string) { const result = await save({ theme_variant: variant }); setState(result); if (result.saved) onSaved?.(); }
  return <Section title="Page colors" intro="Your page always wears your faction's colors. Pick the variant that suits you; the rest of the site keeps each visitor's own colors.">
    <div className="variant-picker" role="radiogroup" aria-label="Color variant">{options.map(v => <button key={v || "default"} type="button" role="radio" aria-checked={data.theme_variant === v} className="variant-choice" data-theme={data.faction ?? undefined} data-variant={v || undefined} onClick={() => void pick(v)}>
      <span className="variant-swatch" aria-hidden="true"><i /><i /><i /></span>{v ? VARIANT_NAMES[v] : DEFAULT_NAMES[data.faction ?? ""] ?? "Default"}</button>)}</div>
    <Status state={state} />
  </Section>;
}

/** Stream first or persona first, and the order of Home's sections. */
export function HomeEditor({ onSaved }: { onSaved?: () => void }) {
  const { data, save } = usePersona();
  const [order, setOrder] = useState<HomeSection[] | null>(null);
  const [state, setState] = useState<SaveState>({});
  if (!data) return null;
  const list = order ?? data.home_order;
  const move = (i: number, by: number) => { const n = [...list]; [n[i], n[i + by]] = [n[i + by], n[i]]; setOrder(n); };
  async function saveLayout(layout: Persona["page_layout"]) { const result = await save({ page_layout: layout }); setState(result); if (result.saved) onSaved?.(); }
  async function saveOrder() { const result = await save({ home_order: list }); setState(result); if (result.saved) { setOrder(null); onSaved?.(); } }
  return <Section title="Your Home" intro="Choose what visitors see first, and the order of Home's sections. Empty sections stay hidden from visitors.">
    <fieldset className="stack"><legend>While you&apos;re live</legend>
      {([["", "Automatic: stream first once you've streamed"], ["stream", "Stream first: your player right under your header"], ["persona", "Persona first: a Live now card, then your Home"]] as const).map(([v, label]) =>
        <label key={v} className="checkbox"><input type="radio" name="layout" checked={data.page_layout === v} onChange={() => void saveLayout(v)} /> {label}</label>)}
    </fieldset>
    <ol className="section-order">{list.map((key, i) => <li key={key}><span>{SECTION_NAMES[key]}</span>
      <button type="button" className="small quiet" disabled={i === 0} onClick={() => move(i, -1)} aria-label={`Move ${SECTION_NAMES[key]} up`}>Up</button>
      <button type="button" className="small quiet" disabled={i === list.length - 1} onClick={() => move(i, 1)} aria-label={`Move ${SECTION_NAMES[key]} down`}>Down</button></li>)}</ol>
    {order && <button type="button" className="small" onClick={() => void saveOrder()}>Save order</button>}
    <Status state={state} />
  </Section>;
}
