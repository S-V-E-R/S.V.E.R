"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../lib/client-api";

type Home = {
  take_down: { open: number; overdue: number; next_deadline: string | null };
  counts: { reports: number; appeals: number; copyright: number; integrity: number; media: number; live: number };
  jobs_stuck: number; paused: string[];
};

/** The staff console's home (docs/ADMIN.md): only what needs attention to run things. */
export default function AdminHome() {
  const [home, setHome] = useState<Home | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<Home>("GET", "/api/admin/home");
    if (r.ok) setHome(r.data); else setError(r.error);
  }, []);
  useLoad(load);
  if (error) return <p role="alert" className="error">{error}</p>;
  if (!home) return <p className="loading">Loading…</p>;
  const td = home.take_down;
  const tiles: [string, string, number, string?][] = [
    ["/admin/take-it-down", "Take It Down requests open", td.open, td.overdue ? `${td.overdue} past the 48-hour deadline` : td.next_deadline ? `Next deadline ${new Date(td.next_deadline).toLocaleString()}` : undefined],
    ["/admin/reports", "Reports open", home.counts.reports],
    ["/admin/appeals", "Appeals waiting", home.counts.appeals],
    ["/admin/copyright", "Copyright cases open", home.counts.copyright],
    ["/admin/integrity", "Viewer integrity cases", home.counts.integrity],
    ["/admin/media", "Emotes awaiting review", home.counts.media],
    ["/admin/jobs", "Stuck or failed jobs", home.jobs_stuck],
    ["/admin/streams", "Live now", home.counts.live],
  ];
  return <section className="panel section"><h1>Staff console</h1>
    {td.overdue > 0 && <p role="alert" className="error"><strong>{td.overdue} Take It Down {td.overdue === 1 ? "request is" : "requests are"} past the legal deadline.</strong> <Link href="/admin/take-it-down">Review now</Link></p>}
    {home.paused.length > 0 && <p role="status"><strong>Switched off:</strong> {home.paused.join(", ")}. <Link href="/admin/switches">Switches</Link></p>}
    <ul className="dock-tiles admin-tiles">{tiles.map(([href, label, n, note]) => <li key={href}>
      <Link href={href} className="dock-tile"><span className="dock-tile-name">{n.toLocaleString()}</span><span>{label}</span>{note && <span className="muted small">{note}</span>}</Link>
    </li>)}</ul>
  </section>;
}
