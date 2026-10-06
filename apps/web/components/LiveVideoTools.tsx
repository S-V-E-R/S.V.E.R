"use client";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import type { VideoPage } from "../lib/videos";
import { VideoCut } from "./VideoCut";

type Recording = { id: string; enabled: boolean; duration_ms: number; available_from_ms: number | null };
export function LiveVideoTools({ username, signedIn, rewind, onRewind }: { username: string; signedIn: boolean; rewind: boolean; onRewind: (id: string | null) => void }) {
  const [recording, setRecording] = useState<Recording | null>(null);
  const [clip, setClip] = useState<VideoPage | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => { const r = await send<{ recording: Recording | null }>("GET", `/api/channels/${encodeURIComponent(username)}/recording`); if (r.ok) setRecording(r.data.recording); }, [username]);
  useLoad(load);
  useEffect(() => { const timer = setInterval(() => void load(), 15000); return () => clearInterval(timer); }, [load]);
  if (!recording) return null;
  return <div className="live-video-tools"><div className="video-actions">
    {recording.enabled && <button type="button" className="small quiet" onClick={() => onRewind(rewind ? null : recording.id)}>{rewind ? "Back to live" : "Rewind broadcast"}</button>}
    {signedIn && recording.duration_ms >= 5000 && <button type="button" className="small quiet" onClick={async () => {
      if (clip) { setClip(null); return; }
      const r = await send<VideoPage>("GET", `/api/videos/${recording.id}`);
      if (r.ok) setClip(r.data); else setMessage(r.error);
    }}>{clip ? "Close clip editor" : "Clip this moment"}</button>}
  </div>{clip && <VideoCut video={clip.video} />}{message && <p role="status">{message}</p>}</div>;
}
