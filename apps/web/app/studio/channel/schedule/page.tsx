"use client";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Block = { weekday: number; start: string; end: string; label: string };
type Event = { title: string; start_at: string; end_at: string };
const days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const zones = typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : ["UTC"];
const local = (iso: string) => { const d = new Date(iso); return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16); };

export default function ScheduleStudio() {
  const [zone, setZone] = useState("");
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [events, setEvents] = useState<Event[]>([]);
  const [revision, setRevision] = useState<number>();
  const [ready, setReady] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<{ timezone: string | null; blocks: Block[]; events: Event[]; revision: number }>("GET", "/api/me/schedule");
    if (!r.ok) return;
    setZone(r.data.timezone || Intl.DateTimeFormat().resolvedOptions().timeZone);
    setBlocks(r.data.blocks); setEvents(r.data.events.map(e => ({ ...e, start_at: local(e.start_at), end_at: local(e.end_at) }))); setRevision(r.data.revision); setReady(true);
  }, []);
  useLoad(load);
  async function save() {
    const result = await send("PUT", "/api/me/schedule", { timezone: zone, blocks, events: events.map(e => ({ title: e.title, start_at: new Date(e.start_at).toISOString(), end_at: new Date(e.end_at).toISOString() })), revision });
    setState(result.ok ? { saved: "Schedule saved." } : result);
    if (result.ok) load();
  }
  if (!ready) return <p className="loading">Loading…</p>;
  const setBlock = (i: number, p: Partial<Block>) => setBlocks(blocks.map((b, j) => j === i ? { ...b, ...p } : b));
  const setEvent = (i: number, p: Partial<Event>) => setEvents(events.map((e, j) => j === i ? { ...e, ...p } : e));
  return <><h1>Schedule</h1>
    <Section title="Time zone" intro="Weekly times follow this zone, including daylight saving changes. Visitors see times in their own zone.">
      <label className="field narrow"><span>Time zone</span><select value={zone} onChange={e => setZone(e.target.value)}>{zones.filter(z => !z.startsWith("Etc/")).map(z => <option key={z} value={z}>{z.replaceAll("_", " ")}</option>)}</select></label>
    </Section>
    <Section title="Weekly blocks" intro="Up to 21 blocks. A block that ends before it starts runs past midnight.">
      {blocks.map((b, i) => <div key={i} className="row">
        <select aria-label="Day" value={b.weekday} onChange={e => setBlock(i, { weekday: Number(e.target.value) })}>{days.map((d, n) => <option key={d} value={n + 1}>{d}</option>)}</select>
        <input aria-label="Start" type="time" value={b.start} onChange={e => setBlock(i, { start: e.target.value })} />
        <input aria-label="End" type="time" value={b.end} onChange={e => setBlock(i, { end: e.target.value })} />
        <input aria-label="Label" value={b.label} maxLength={60} placeholder="Label (optional)" onChange={e => setBlock(i, { label: e.target.value })} />
        <button type="button" className="small quiet" onClick={() => setBlocks(blocks.filter((_, j) => j !== i))}>Remove</button>
      </div>)}
      {blocks.length < 21 && <button type="button" className="small quiet" onClick={() => setBlocks([...blocks, { weekday: 1, start: "19:00", end: "21:00", label: "" }])}>Add block</button>}
    </Section>
    <Section title="One-off events" intro="Up to 20 upcoming events, each up to 24 hours, within the next year.">
      {events.map((e, i) => <div key={i} className="row">
        <input aria-label="Title" value={e.title} maxLength={100} placeholder="Title" onChange={ev => setEvent(i, { title: ev.target.value })} />
        <input aria-label="Starts" type="datetime-local" value={e.start_at} onChange={ev => setEvent(i, { start_at: ev.target.value })} />
        <input aria-label="Ends" type="datetime-local" value={e.end_at} onChange={ev => setEvent(i, { end_at: ev.target.value })} />
        <button type="button" className="small quiet" onClick={() => setEvents(events.filter((_, j) => j !== i))}>Remove</button>
      </div>)}
      {events.length < 20 && <button type="button" className="small quiet" onClick={() => { const s = new Date(Date.now() + 86400000); setEvents([...events, { title: "", start_at: local(s.toISOString()), end_at: local(new Date(s.getTime() + 7200000).toISOString()) }]); }}>Add event</button>}
    </Section>
    <button type="button" onClick={save}>Save schedule</button>
    <Status state={state} /></>;
}
