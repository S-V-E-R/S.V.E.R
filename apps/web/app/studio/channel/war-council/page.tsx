"use client";
import { useCallback, useEffect, useState } from "react";
import { Avatar } from "../../../../components/Avatar";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";
import type { Chip } from "../../../../lib/types";

type Member = { position: number; user: Chip; available: boolean };
export default function CouncilStudio() {
  const [members, setMembers] = useState<Member[] | null>(null);
  const [revision, setRevision] = useState<number>();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Chip[]>([]);
  const [state, setState] = useState<SaveState>({});
  // Short queries show nothing; stale results stay hidden until the next search lands.
  const shown = query.trim().length < 2 ? [] : results;
  const load = useCallback(async () => { const r = await send<{ members: Member[]; revision: number }>("GET", "/api/me/war-council"); if (r.ok) { setMembers(r.data.members); setRevision(r.data.revision); } }, []);
  useLoad(load);
  useEffect(() => {
    if (query.trim().length < 2) return;
    const timer = setTimeout(async () => { const r = await send<{ results: Chip[] }>("GET", `/api/me/war-council/search?q=${encodeURIComponent(query.trim())}`); if (r.ok) setResults(r.data.results); }, 300);
    return () => clearTimeout(timer);
  }, [query]);
  if (!members) return <p className="loading">Loading…</p>;
  const move = (i: number, d: number) => { const next = [...members]; [next[i], next[i + d]] = [next[i + d], next[i]]; setMembers(next); };
  async function save() {
    const result = await send<{ revision: number }>("PUT", "/api/me/war-council", { members: members!.map(m => m.user.username).filter(Boolean), revision });
    setState(result.ok ? { saved: "War Council saved." } : result);
    if (result.ok) load();
  }
  return <><h1>War Council</h1>
    <Section title="Your Top 8" intro="Pick up to 8 channels and put them in order. Members who become unavailable are hidden from visitors.">
      <ol className="list">{members.map((m, i) => <li key={m.user.username ?? i} className="row between">
        <span className="row"><Avatar sizes={m.user.avatar} name={m.user.display_name} size={32} /> {m.user.display_name} {m.user.username && <span className="handle">@{m.user.username}</span>}{!m.available && <span className="badge">Unavailable</span>}</span>
        <span className="row"><button type="button" className="small quiet" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up">↑</button><button type="button" className="small quiet" disabled={i === members.length - 1} onClick={() => move(i, 1)} aria-label="Move down">↓</button><button type="button" className="small quiet" onClick={() => setMembers(members.filter((_, j) => j !== i))}>Remove</button></span>
      </li>)}</ol>
      {members.length < 8 ? <label className="field"><span>Add a channel</span><input value={query} onChange={e => setQuery(e.target.value)} placeholder="Search by username" /></label> : <p className="muted">Your War Council is full.</p>}
      {shown.length > 0 && <ul className="list">{shown.filter(r => !members.some(m => m.user.username === r.username)).map(r => <li key={r.username}><button type="button" className="link-button" onClick={() => { setMembers([...members, { position: members.length + 1, user: r, available: true }]); setQuery(""); }}>{r.display_name} @{r.username}</button></li>)}</ul>}
      <button type="button" className="small" onClick={save}>Save War Council</button>
      <Status state={state} />
    </Section></>;
}
