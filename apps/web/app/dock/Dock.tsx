"use client";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { GamePicker } from "../../components/GamePicker";
import { Uptime } from "../../components/PlayerSettings";
import { send } from "../../lib/client-api";
import { fromBrowser } from "../../lib/languages";
import { onAir } from "../../lib/obs";

type Settings = { title: string; category_id: string | null; revision: number; mature?: boolean; language?: string | null };
type Health = { input_kbps?: number | null; codec_warning?: boolean; bitrate_warning?: boolean; keyframe_seconds?: number | null; keyframe_warning?: boolean; b_frames?: boolean | null };
type Stream = { eligible: boolean; settings: Settings; disconnect_pending: boolean; credential: { revoked: boolean } | null; broadcast: { state: string; started_at: string; observed_at: string | null; health: Health } | null };
type Overlay = { stream: { viewerCount: number } | null; stats: { followerCount: number } };
const ALERTS = [["follow", "Follow"], ["sub", "Subscription"], ["tribute", "Tribute"], ["skill", "Skill"], ["raid", "Raid"]] as const;

/** The OBS dock: on-air state, viewers and input health, title and category, a highlight marker and alert tests. */
export function Dock({ username }: { username: string }) {
  const [data, setData] = useState<Stream | null>(null);
  const [seenAt, setSeenAt] = useState(0);
  const [counts, setCounts] = useState<Overlay | null>(null);
  const [form, setForm] = useState<Settings | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const [stream, overlay] = await Promise.all([send<Stream>("GET", "/api/me/stream"), send<Overlay>("GET", `/api/channels/${encodeURIComponent(username)}/overlay`)]);
    if (stream.ok) { setData(stream.data); setSeenAt(Date.now()); setForm(previous => previous ?? stream.data.settings); } else setNote(stream.error);
    if (overlay.ok) setCounts(overlay.data);
  }, [username]);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- load awaits the network before setting state
    void load();
    let pending = false;
    const timer = setInterval(async () => {
      if (pending || document.hidden) return;
      pending = true;
      try { await load(); } finally { pending = false; }
    }, 5000);
    return () => clearInterval(timer);
  }, [load]);
  async function act(run: () => Promise<{ ok: boolean; error?: string }>, done: string) {
    if (busy) return;
    setBusy(true); setNote("");
    const result = await run();
    setNote(result.ok ? done : result.error ?? "Please try again.");
    setBusy(false);
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!form) return;
    await act(async () => {
      const result = await send<{ revision: number }>("PATCH", "/api/me/stream", { ...form, language: form.language ?? fromBrowser(navigator.languages)[0] ?? "other" });
      if (result.ok) setForm({ ...form, revision: result.data.revision });
      return result;
    }, "Title and category saved.");
  }
  if (!data || !form) return <p className={note ? "form-message error" : "loading"} role={note ? "alert" : undefined}>{note || "Loading…"}</p>;
  const broadcast = data.broadcast;
  const health = broadcast?.health ?? {};
  const air = onAir(broadcast, data.disconnect_pending, !!data.credential && !data.credential.revoked, seenAt);
  const count = Array.from(form.title.trim()).length;
  return <>
    <section className="panel obs-dock-status" aria-label="On air">
      <p role="status"><span className={air.live ? "live badge" : "badge"}>{air.live ? "Live" : air.state}</span> {air.plain}</p>
      <p className="small-text">{counts?.stream ? <>{counts.stream.viewerCount.toLocaleString()} watching · </> : null}{counts ? <>{counts.stats.followerCount.toLocaleString()} followers</> : null}{broadcast && broadcast.state !== "ENDED" && <> · <Uptime since={broadcast.started_at} /></>}{health.input_kbps != null && <> · {Math.round(health.input_kbps).toLocaleString()} Kbps</>}</p>
      {health.codec_warning && <p role="alert">Video or audio isn&apos;t H.264 and AAC. Check OBS&apos;s encoder settings.</p>}
      {health.keyframe_warning && <p role="alert">Keyframes are about {health.keyframe_seconds} seconds apart. Set Keyframe Interval to 1 s.</p>}
      {health.b_frames && <p role="alert">B-frames are on. Set them to 0.</p>}
      {health.bitrate_warning && <p role="alert">Bitrate is over 8 Mbps. Lower it if playback is unstable.</p>}
    </section>
    {!data.eligible && <p className="panel notice">Streaming needs a verified email, an authenticator and a channel in good standing. <a href="/account" target="_blank" rel="noreferrer">Account security</a></p>}
    {note && <p role="status" className="form-message">{note}</p>}
    <form className="panel stack" onSubmit={save} aria-label="Title and category">
      <label className="field"><span>Title</span><input required maxLength={280} value={form.title} disabled={busy || !data.eligible} aria-describedby="dock-title-count" onChange={e => setForm({ ...form, title: e.target.value })} /><small id="dock-title-count">{count}/140</small></label>
      <GamePicker value={form.category_id ?? ""} disabled={busy || !data.eligible} onChange={(category_id, mature) => setForm({ ...form, category_id, mature: mature || form.mature })} />
      <div className="row wrap"><button className="small" disabled={busy || !data.eligible || count < 1 || count > 140}>Save</button><button type="button" className="small quiet" disabled={busy} onClick={() => { setForm(data.settings); setNote(""); }}>Undo changes</button></div>
    </form>
    <section className="panel stack" aria-label="Actions">
      <button type="button" className="small" disabled={busy || !air.live} onClick={() => act(() => send("POST", `/api/channels/${encodeURIComponent(username)}/marker`, {}), "Highlight marked.")}>Mark a highlight</button>
      <form className="row wrap" onSubmit={e => { e.preventDefault(); const kind = new FormData(e.currentTarget).get("kind"); void act(() => send("POST", "/api/me/overlays/test", { kind }), "Test sent to your overlay."); }}>
        <label className="field narrow"><span>Test alert</span><select name="kind">{ALERTS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select></label>
        <button className="small quiet" disabled={busy}>Send</button>
      </form>
      <p className="small-text"><a href="/studio/stream" target="_blank" rel="noreferrer">Open Creator Studio</a></p>
    </section>
  </>;
}
