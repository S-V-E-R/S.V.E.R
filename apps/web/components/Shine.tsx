"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { ReportButton } from "./Report";
import type { Chip } from "../lib/types";

type Moment = { video_id: string; kind: "CLIP" | "HIGHLIGHT"; title: string; description: string };
type Lights = { count: number; recent: { id: string; from: Chip; message: string; at: string; can_report?: boolean }[] };

/** Shine Moments and Light It Up on a channel page (docs/SUPPORT.md "Shine"). */
export function Shine({ username, isOwner, signedIn }: { username: string; isOwner: boolean; signedIn: boolean }) {
  const [moments, setMoments] = useState<Moment[]>([]);
  const [lights, setLights] = useState<Lights | null>(null);
  const [message, setMessage] = useState("");
  const [giving, setGiving] = useState(false);
  const load = useCallback(async () => {
    const [m, l] = await Promise.all([send<{ moments: Moment[] }>("GET", `/api/channels/${encodeURIComponent(username)}/moments`), send<Lights>("GET", `/api/channels/${encodeURIComponent(username)}/lights`)]);
    if (m.ok) setMoments(m.data.moments);
    if (l.ok) setLights(l.data);
  }, [username]);
  useLoad(load);
  async function give(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    if (giving) return;
    setGiving(true);
    const r = await send<{ left_today: number }>("POST", "/api/lights", { to: username, message: new FormData(form).get("message") });
    setGiving(false);
    if (r.ok) { setMessage(`Lit up. ${r.data.left_today} left today.`); form.reset(); await load(); } else setMessage(r.error);
  }
  async function hide(id: string) {
    const r = await send("DELETE", `/api/me/lights/${encodeURIComponent(id)}`);
    if (r.ok) await load(); else setMessage(r.error);
  }
  if (!moments.length && !lights?.count && (isOwner || !signedIn)) return null;
  return <section className="panel section shine" aria-labelledby="shine-title">
    <div className="row between"><h2 id="shine-title">Shine</h2>{!!lights?.count && <span className="muted small">{lights.count.toLocaleString()} {lights.count === 1 ? "light" : "lights"}</span>}</div>
    {!!moments.length && <ol className="moment-list">{moments.map(m => <li key={m.video_id}><Link href={`/${m.kind === "CLIP" ? "clips" : "videos"}/${m.video_id}`}><strong>{m.title}</strong></Link>{m.description && <p className="muted small">{m.description}</p>}</li>)}</ol>}
    {signedIn && !isOwner && <p className="muted small">Send a light to say this channel did something good. It&apos;s free: 3 a day, one per channel.</p>}
    {signedIn && !isOwner && <form onSubmit={give} className="row wrap">
      <label className="field"><span className="sr-only">Message only this channel sees (optional)</span><input name="message" maxLength={200} placeholder="A few words only they will see (optional)" /></label>
      <button type="submit" className="small" disabled={giving}>Light it up</button>
    </form>}
    {message && <p role="status" className="muted small">{message}</p>}
    {!!lights?.recent.length && <ul className="light-list">{lights.recent.map(l => <li key={l.id}><strong>{l.from.display_name}</strong>: {l.message}{isOwner && <> <button type="button" className="small quiet" onClick={() => void hide(l.id)}>Hide</button></>}{l.can_report && <> <ReportButton target={{ target_type: "light", target_id: l.id }} /></>}</li>)}</ul>}
  </section>;
}
