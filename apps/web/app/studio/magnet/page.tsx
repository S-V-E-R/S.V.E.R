"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Feature = { lane: string; name: string; kind: string; reason: string; started_at: string; ended_at: string | null; seconds: number; hype_viewers: number; followed: number; chatted: number };
type Mine = { opt_out: boolean; chat_merge: boolean; featured_now: { lane: string; name: string; reason: string; since: string }[]; history: Feature[] };
const minutes = (s: number) => s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;

/** Creator Studio → MAGNet (docs/MAGNET.md "What the streamer sees"). */
export default function StudioMagnet() {
  const [data, setData] = useState<Mine | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await send<Mine>("GET", "/api/me/magnet");
    if (result.ok) setData(result.data); else setMessage(result.error);
  }, []);
  useLoad(load);
  async function save(change: Partial<Mine>) {
    if (!data) return;
    const next = { ...data, ...change };
    const result = await send<Mine>("PUT", "/api/me/magnet", { opt_out: next.opt_out, chat_merge: next.chat_merge });
    if (result.ok) { setData(result.data); setMessage("Saved."); } else setMessage(result.error);
  }
  async function flag() {
    const result = await send("POST", "/api/me/magnet/flag");
    setMessage(result.ok ? "Moment flagged. It counts when your chat or follows are also up." : result.error);
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>MAGNet</h1>
    <Section title="Right now" intro="MAGNet features streams that are having a moment and gives every stream a fair turn. Viewer counts, followers and money never affect it.">
      {data.featured_now.length ? <ul className="list">{data.featured_now.map(f => <li key={f.lane}>Featured on <Link href={f.lane === "global" ? "/magnet" : `/magnet/${f.lane}`}>{f.name}</Link> · {f.reason}</li>)}</ul> : <p className="muted">You aren&apos;t featured right now.</p>}
      <button type="button" className="small" onClick={flag}>Flag this moment</button> <span className="muted">Also <code>/flag</code> in your chat; once every 10 minutes.</span>
    </Section>
    <Section title="Settings">
      <label className="checkbox"><input type="checkbox" checked={!data.opt_out} onChange={e => save({ opt_out: !e.target.checked })} /> Feature my streams in MAGNet</label>
      <label className="checkbox"><input type="checkbox" checked={data.chat_merge} onChange={e => save({ chat_merge: e.target.checked })} /> Merge MAGNet chat into my chat while I&apos;m featured</label>
    </Section>
    <Section title="Feature history" intro="The last 30 days. Who stayed to follow or chat is shown to you only and never used for scoring.">
      {data.history.length === 0 ? <p className="muted">No features yet.</p> : <table className="table"><thead><tr><th>Started</th><th>Lane</th><th>Why</th><th>Length</th><th>MAGNet viewers</th><th>Followed</th><th>Chatted</th></tr></thead>
        <tbody>{data.history.map(f => <tr key={f.started_at + f.lane}><td>{new Date(f.started_at).toLocaleString()}</td><td>{f.name}</td><td>{f.reason}</td><td>{minutes(f.seconds)}</td><td>{f.hype_viewers}</td><td>{f.followed}</td><td>{f.chatted}</td></tr>)}</tbody></table>}
    </Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
