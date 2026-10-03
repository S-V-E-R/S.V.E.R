"use client";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { UserChip } from "../../../../components/UserChip";
import { send, useLoad } from "../../../../lib/client-api";
import type { Chip } from "../../../../lib/types";

type Pending = { id: string; image: Record<string, string>; artist_name: string; artist_link: string | null; caption: string; submitted_at: string; submitter: Chip };
export default function FanArtStudio() {
  const [data, setData] = useState<{ enabled: boolean; pending: Pending[]; approved_count: number } | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<NonNullable<typeof data>>("GET", "/api/me/fan-art"); if (r.ok) setData(r.data); }, []);
  useLoad(load);
  async function toggle(enabled: boolean) { const r = await send("PUT", "/api/me/fan-art/settings", { enabled }); setState(r.ok ? { saved: enabled ? "Fan art is on." : "Fan art is off." } : r); load(); }
  async function review(id: string, action: "approve" | "reject") { const r = await send("POST", `/api/fan-art/${id}/${action}`); setState(r.ok ? { saved: action === "approve" ? "Approved." : "Rejected." } : r); load(); }
  if (!data) return <p className="loading">Loading…</p>;
  return <><h1>Fan Art</h1>
    <Section title="Fan art submissions" intro="When on, signed-in viewers with a verified email can submit art to your Fan Art tab. Nothing shows until you approve it. Up to 20 can wait for review and 100 can be shown.">
      <label className="checkbox"><input type="checkbox" checked={data.enabled} onChange={e => toggle(e.target.checked)} /> Accept fan art</label>
      <p className="muted">{data.approved_count} approved.</p>
    </Section>
    <Section title={`Waiting for approval (${data.pending.length})`}>
      {data.pending.length === 0 ? <p className="muted">Nothing waiting.</p> : <ul className="gallery">{data.pending.map(p => <li key={p.id} className="panel">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={p.image["400"]} alt={p.caption || `Fan art by ${p.artist_name}`} width={400} />
        <p>{p.artist_name}{p.caption && <span className="muted"> — {p.caption}</span>}</p>
        <UserChip user={p.submitter} size={20} />
        <div className="row"><button type="button" className="small" onClick={() => review(p.id, "approve")}>Approve</button><button type="button" className="small quiet" onClick={() => review(p.id, "reject")}>Reject</button></div>
      </li>)}</ul>}
      <Status state={state} />
    </Section></>;
}
