"use client";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Kind = "ABOUT" | "PANEL" | "QUOTES" | "GAME_SHELF" | "PROMPT";
type Block = { type: Kind; enabled: boolean; config: { body?: string; title?: string; quotes?: { text: string; attribution: string }[]; games?: string[]; question?: string; answer?: string } };
/** The fixed MySpace-style questions (docs/PROFILES.md "Persona pass"); each can be answered once. */
const PROMPTS: [string, string][] = [["meet", "Who I'd like to meet"], ["made_me", "Games that made me"], ["hot_take", "My hot take"], ["obsessed", "Currently obsessed with"], ["moment", "Best stream moment"], ["chat_calls_me", "What my chat calls me"]];
const names: Record<Kind, string> = { ABOUT: "About text", PANEL: "Panel", QUOTES: "Quotes", GAME_SHELF: "Game shelf", PROMPT: "Question" };
const blank = (type: Kind): Block => ({ type, enabled: true, config: type === "ABOUT" ? { body: "" } : type === "PANEL" ? { title: "", body: "" } : type === "QUOTES" ? { quotes: [{ text: "", attribution: "" }] } : type === "PROMPT" ? { question: "", answer: "" } : { games: [""] } });

export default function BlocksStudio() {
  const [items, setItems] = useState<Block[] | null>(null);
  const [revision, setRevision] = useState<number>();
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<{ items: Block[]; revision: number }>("GET", "/api/me/page-blocks"); if (r.ok) { setItems(r.data.items); setRevision(r.data.revision); } }, []);
  useLoad(load);
  if (!items) return <p className="loading">Loading…</p>;
  const set = (i: number, config: Block["config"]) => setItems(items.map((b, j) => j === i ? { ...b, config } : b));
  async function save() {
    const result = await send("PUT", "/api/me/blocks", { items, revision });
    setState(result.ok ? { saved: "Blocks saved." } : result);
    if (result.ok) load();
  }
  const asked = items.filter(b => b.type === "PROMPT").map(b => b.config.question);
  return <><h1>About me</h1>
    <Section title="Blocks" intro="Up to 10 blocks in the About me section of your Home. Questions are MySpace-style prompts you answer in your own words. Text supports **bold**, *italic*, [links](https://…) and lists starting with “- ”.">
      {items.map((b, i) => <fieldset key={i} className="panel editor">
        <legend>{names[b.type]}</legend>
        {(b.type === "PANEL") && <label className="field"><span>Title</span><input value={b.config.title || ""} maxLength={80} onChange={e => set(i, { ...b.config, title: e.target.value })} /></label>}
        {(b.type === "ABOUT" || b.type === "PANEL") && <label className="field"><span>Text</span><textarea value={b.config.body || ""} maxLength={2000} rows={5} onChange={e => set(i, { ...b.config, body: e.target.value })} /></label>}
        {b.type === "PROMPT" && <>
          <label className="field"><span>Question</span><select value={b.config.question || ""} onChange={e => set(i, { ...b.config, question: e.target.value })}><option value="">Choose a question</option>{PROMPTS.map(([k, q]) => <option key={k} value={k} disabled={k !== b.config.question && asked.includes(k)}>{q}</option>)}</select></label>
          <label className="field"><span>Your answer</span><textarea value={b.config.answer || ""} maxLength={500} rows={3} onChange={e => set(i, { ...b.config, answer: e.target.value })} /></label>
        </>}
        {b.type === "QUOTES" && <>{b.config.quotes!.map((q, j) => <div key={j} className="row"><input aria-label="Quote" value={q.text} maxLength={280} placeholder="Quote" onChange={e => set(i, { quotes: b.config.quotes!.map((x, k) => k === j ? { ...x, text: e.target.value } : x) })} /><input aria-label="Attribution" value={q.attribution} maxLength={80} placeholder="Who said it" onChange={e => set(i, { quotes: b.config.quotes!.map((x, k) => k === j ? { ...x, attribution: e.target.value } : x) })} /></div>)}{b.config.quotes!.length < 5 && <button type="button" className="small quiet" onClick={() => set(i, { quotes: [...b.config.quotes!, { text: "", attribution: "" }] })}>Add quote</button>}</>}
        {b.type === "GAME_SHELF" && <>{b.config.games!.map((g, j) => <input key={j} aria-label="Game" value={g} maxLength={80} placeholder="Game" onChange={e => set(i, { games: b.config.games!.map((x, k) => k === j ? e.target.value : x) })} />)}{b.config.games!.length < 12 && <button type="button" className="small quiet" onClick={() => set(i, { games: [...b.config.games!, ""] })}>Add game</button>}</>}
        <div className="row"><label className="checkbox"><input type="checkbox" checked={b.enabled} onChange={e => setItems(items.map((x, j) => j === i ? { ...x, enabled: e.target.checked } : x))} /> Show</label>
          <button type="button" className="small quiet" disabled={i === 0} onClick={() => { const n = [...items]; [n[i - 1], n[i]] = [n[i], n[i - 1]]; setItems(n); }}>Move up</button>
          <button type="button" className="small quiet" onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button></div>
      </fieldset>)}
      {items.length < 10 && <div className="row">{(Object.keys(names) as Kind[]).map(k => <button key={k} type="button" className="small quiet" onClick={() => setItems([...items, blank(k)])}>Add {names[k].toLowerCase()}</button>)}</div>}
      <button type="button" className="small" onClick={save}>Save blocks</button>
      <Status state={state} />
    </Section></>;
}
