"use client";
import Link from "next/link";
import { useRef, useState } from "react";
import { send } from "../lib/client-api";
import { duration, type Video } from "../lib/videos";

export function VideoCut({ video, highlight = false, ageAck = false }: { video: Video; highlight?: boolean; ageAck?: boolean }) {
  const minimum = video.status === "RECORDING" && !highlight ? Math.max(0, video.duration_ms - 120000) : 0;
  const [start, setStart] = useState(Math.max(minimum, video.duration_ms - 30000));
  const [end, setEnd] = useState(video.duration_ms);
  const [message, setMessage] = useState("");
  const [saved, setSaved] = useState("");
  const [busy, setBusy] = useState(false);
  const request = useRef("");
  const valid = start >= minimum && end <= video.duration_ms && end > start && (highlight || (end - start >= 5000 && end - start <= 60000));
  return <form className="video-cut panel" onSubmit={async e => {
    e.preventDefault(); if (busy || !valid) return;
    if (!request.current) request.current = crypto.randomUUID();
    setBusy(true);
    const result = await send<{ id: string; start_ms: number; end_ms: number }>("POST", `/api/videos/${video.id}/cuts`, { kind: highlight ? "HIGHLIGHT" : "CLIP", title: new FormData(e.currentTarget).get("title"), start_ms: start, end_ms: end, request_id: request.current, age_ack: ageAck });
    setBusy(false);
    if (result.ok) { setSaved(result.data.id); setMessage("Saved. Your video is processing."); } else { setMessage(result.error); }
  }}>
    <h2>{highlight ? "Save a Highlight" : "Create a clip"}</h2>
    <p className="muted">{highlight ? "Keep this section permanently. It counts toward your Highlight storage." : "Choose 5–60 seconds."} Cuts snap to the nearest available segment.</p>
    <label className="field"><span>Title</span><input name="title" required maxLength={highlight ? 140 : 100} defaultValue={video.title.slice(0, highlight ? 140 : 100)} disabled={!!saved} /></label>
    <label className="field"><span>Start · {duration(start)}</span><input type="range" min={minimum} max={video.duration_ms} step={1000} value={start} onChange={e => { setStart(Number(e.target.value)); request.current = ""; }} disabled={!!saved} /></label>
    <label className="field"><span>End · {duration(end)}</span><input type="range" min={minimum} max={video.duration_ms} step={1000} value={end} onChange={e => { setEnd(Number(e.target.value)); request.current = ""; }} disabled={!!saved} /></label>
    <p>{duration(Math.max(0, end - start))} selected</p>
    {!saved && <button disabled={busy || !valid}>{busy ? "Saving…" : highlight ? "Save Highlight" : "Create clip"}</button>}
    {message && <p role="status">{message} {saved && <Link href={`/${highlight ? "videos" : "clips"}/${saved}`}>Open {highlight ? "Highlight" : "clip"}</Link>}</p>}
  </form>;
}
