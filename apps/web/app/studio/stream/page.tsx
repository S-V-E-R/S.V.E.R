"use client";
import Link from "next/link";
import { LANGUAGES, fromBrowser } from "../../../lib/languages";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { GamePicker } from "../../../components/GamePicker";

/** A new channel's language defaults to the browser's (then "other"). */
const defaultLanguage = () => fromBrowser(typeof navigator === "undefined" ? [] : navigator.languages)[0] ?? "other";
type Settings = { title: string; category_id: string | null; revision: number; mature?: boolean; mature_locked?: boolean; language?: string | null; edited_by?: string | null };
type Health = { video_codec?: string | null; audio_codec?: string | null; width?: number | null; height?: number | null; input_kbps?: number | null; codec_warning?: boolean; bitrate_warning?: boolean; keyframe_seconds?: number | null; keyframe_warning?: boolean; b_frames?: boolean | null; captions?: boolean };
type Stream = {
  configured: boolean; eligible: boolean; settings: Settings; disconnect_pending: boolean;
  last_broadcast?: { ended_at: string; not_counted: number } | null;
  credential: { created_at: string; revoked: boolean } | null;
  broadcast: { state: string; started_at: string; reconnect_deadline: string | null; observed_at: string | null; end_reason?: string | null; health: Health; scouts?: number } | null;
};
type Account = { has_password: boolean; reauthenticated: boolean };
type Key = { server: string; key: string; whip?: { url: string; token: string } | null; srt?: string | null; disconnect_pending: boolean };

export default function StreamStudio() {
  const [data, setData] = useState<Stream | null>(null);
  // When data last arrived; staleness is judged against it so render stays pure.
  const [seenAt, setSeenAt] = useState(0);
  const [form, setForm] = useState<Settings | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [status, setStatus] = useState<SaveState>({});
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [secret, setSecret] = useState<Key | null>(null);
  const [rotate, setRotate] = useState(false);
  const [ratedMature, setRatedMature] = useState(false);
  const [ingest, setIngest] = useState<"rtmp" | "whip" | "srt">("rtmp");
  const active = useRef(true);
  const secretEpoch = useRef(0);
  const load = useCallback(async () => {
    const result = await send<Stream>("GET", "/api/me/stream");
    if (!active.current) return;
    if (result.ok) {
      setData(result.data);
      setSeenAt(Date.now());
      // Polling must not overwrite unsaved input or advance its conflict revision.
      setForm(previous => previous ?? result.data.settings);
      setLoadError("");
      if (result.data.credential?.revoked || !result.data.eligible || !result.data.configured) setSecret(null);
    } else { setLoadError(result.error); setSecret(null); }
  }, []);
  useEffect(() => {
    active.current = true;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- load awaits the network before setting state
    load();
    send<Account>("GET", "/api/auth/me").then(me => {
      if (!active.current) return;
      if (me.ok) setAccount(me.data);
    });
    let pending = false;
    const timer = setInterval(async () => {
      if (pending || document.hidden) return;
      pending = true;
      try { await load(); } finally { pending = false; }
    }, 5000);
    const hide = () => { if (document.hidden) { secretEpoch.current++; setSecret(null); setPassword(""); setCode(""); } };
    document.addEventListener("visibilitychange", hide);
    return () => { active.current = false; clearInterval(timer); document.removeEventListener("visibilitychange", hide); };
  }, [load]);
  useEffect(() => {
    if (!secret) return;
    const timer = setTimeout(() => setSecret(null), 60000);
    return () => clearTimeout(timer);
  }, [secret]);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!form || busy) return;
    setBusy(true); setStatus({});
    const result = await send<{ revision: number; category_id: string }>("PATCH", "/api/me/stream", { ...form, language: form.language ?? defaultLanguage() });
    if (result.ok) {
      setForm(previous => previous && { ...previous, revision: result.data.revision, category_id: result.data.category_id ?? previous.category_id });
      setStatus({ saved: "Stream details saved." }); await load();
    } else setStatus(result);
    setBusy(false);
  }
  async function reloadDetails() {
    const result = await send<Stream>("GET", "/api/me/stream");
    if (result.ok) { setForm(result.data.settings); setData(result.data); setSeenAt(Date.now()); setStatus({}); }
    else setStatus(result);
  }
  async function keyAction(action: "create" | "reveal" | "rotate") {
    if (busy) return;
    const epoch = ++secretEpoch.current;
    setBusy(true); setSecret(null); setStatus({});
    try {
      if (password) {
        const primary = await send("POST", "/api/auth/reauth", { password });
        setPassword("");
        if (!primary.ok) { setStatus(primary); return; }
        setAccount(previous => previous && { ...previous, reauthenticated: true });
      }
      const result = await send<Key>("POST", "/api/me/stream/key" + (action === "create" ? "" : "/" + action), { code });
      setCode("");
      if (result.ok) {
        if (active.current && !document.hidden && epoch === secretEpoch.current) setSecret(result.data);
        setRotate(false);
        setStatus({ saved: result.data.disconnect_pending ? "New key ready. The previous publisher is still disconnecting." : "Key ready. It will hide after one minute." });
        await load();
      } else setStatus(result);
    } finally { setBusy(false); }
  }
  async function stop() {
    setBusy(true); setSecret(null); setStatus({});
    const result = await send<{ disconnect_pending: boolean }>("POST", "/api/me/stream/stop");
    setStatus(result.ok ? { saved: result.data.disconnect_pending ? "Key revoked. OBS disconnection is pending; we'll keep retrying." : "Stream stopped and key revoked. Create a new key before broadcasting again." } : result);
    await load(); setBusy(false);
  }
  async function copy(value: string) {
    try { await navigator.clipboard.writeText(value); setStatus({ saved: "Copied." }); }
    catch { setStatus({ error: "Copy is unavailable. Select the field and copy it manually." }); }
  }
  if (!data || !form) return <><h1>Live stream</h1>{loadError ? <><p role="alert">{loadError}</p><button className="quiet" onClick={load}>Retry</button></> : <p className="loading">Loading your stream…</p>}</>;
  const broadcast = data.broadcast;
  const health = broadcast?.health ?? {};
  const staleSignal = broadcast?.state === "LIVE" && (!broadcast.observed_at || seenAt - Date.parse(broadcast.observed_at) > 15000);
  const state = data.disconnect_pending ? "Disconnecting" : staleSignal ? "Signal not confirmed" : ({ STARTING: "Connecting", LIVE: "Live", RECONNECTING: "Reconnecting", ENDED: "Offline" }[broadcast?.state ?? "ENDED"] ?? "Offline");
  const usable = data.configured && data.eligible && !loadError;
  const count = Array.from(form.title.trim()).length;
  return <><span className="eyebrow">CREATOR STUDIO / BROADCAST</span><h1>Live stream</h1>
    {loadError && <p className="panel notice" role="alert">{loadError} Status may be out of date.</p>}
    {!data.configured && <p className="panel notice">Broadcasting is not available yet. You can prepare your stream details below.</p>}
    {!data.eligible && <p className="panel notice">Streaming needs a verified email, an authenticator and a channel in good standing. <Link href="/account">Account security</Link> · <Link href="/settings/standing">Account standing</Link></p>}
    <Status state={status} />
    <Section title="On air">
      <p role="status"><strong>{state}</strong>{broadcast?.state === "RECONNECTING" && broadcast.reconnect_deadline && <> — reconnect OBS before {new Date(broadcast.reconnect_deadline).toLocaleTimeString()} to continue this broadcast.</>}</p>
      {broadcast?.state !== "LIVE" && broadcast?.state !== "RECONNECTING" && data.last_broadcast && data.last_broadcast.not_counted > 0 && <p className="muted">Last stream: {data.last_broadcast.not_counted.toLocaleString()} {data.last_broadcast.not_counted === 1 ? "viewer was" : "viewers were"} not counted. Viewer counts only include people whose playback passes our checks; automated or suspicious traffic is left out and never counts against you.</p>}
      <p>Your stream starts when OBS connects. A dropped connection has 60 seconds to resume the same broadcast.</p>
      {staleSignal && <p role="status">Fresh media has not been confirmed. Check the OBS connection; the last measurements below may be out of date.</p>}
      {broadcast?.end_reason === "startup_timeout" && <p role="alert">Compatible media was not confirmed within 15 seconds. Check your OBS encoder settings and connection before trying again.</p>}
      {data.credential && !data.credential.revoked && <button type="button" className="quiet danger-text" disabled={busy} onClick={stop}>Stop stream and revoke key</button>}
    </Section>
    <Section title="Stream details" intro="Choose a title and category before connecting OBS. You can change them while live.">
      <form onSubmit={save}>
        <label className="field"><span>Title</span><input required maxLength={280} value={form.title} disabled={busy || !data.eligible} aria-describedby="stream-title-count" onChange={event => setForm({ ...form, title: event.target.value })} /><small id="stream-title-count">{count}/140 characters</small></label>
        <GamePicker value={form.category_id ?? ""} disabled={busy || !data.eligible} onChange={(category_id, mature) => { setForm({ ...form, category_id, mature: mature || form.mature }); setRatedMature(mature); }} />
        <label className="field narrow"><span>Language</span><select required value={form.language ?? defaultLanguage()} disabled={busy || !data.eligible} onChange={event => setForm({ ...form, language: event.target.value })}>
          {LANGUAGES.map(([code, name]) => <option key={code} value={code}>{name}</option>)}
        </select></label>
        <label className="checkbox"><input type="checkbox" checked={!!form.mature} disabled={busy || !data.eligible || (form.mature_locked && form.mature)} onChange={event => setForm({ ...form, mature: event.target.checked })} /> <strong>Mature</strong>: violent or horror games, strong language or mature themes. Viewers under 18 can&apos;t watch, and others see a warning first. It doesn&apos;t permit anything the <Link href="/guidelines">Community Guidelines</Link> ban. It stays on for your next streams until you turn it off.</label>
        {ratedMature && form.mature && <p role="status">This game has a mature age rating (ESRB M or AO, or PEGI 18), so Mature is switched on. You can turn it off.</p>}
        {form.edited_by && <p className="muted small">Edited by @{form.edited_by}</p>}
        {form.mature_locked && <p role="status">Staff labeled this broadcast mature, so the label stays on until it ends.</p>}
        <div className="row"><button disabled={busy || !data.eligible || count < 1 || count > 140}>Save details</button><button type="button" className="quiet" disabled={busy} onClick={reloadDetails}>Reload saved details</button></div>
      </form>
    </Section>
    <Editors />
    <Section title="OBS connection" intro="Use Custom in OBS's Stream settings. Keep your stream key private.">
      {(!data.settings.title.trim() || !data.settings.category_id) && <p role="alert">Save a title and category in Stream details first. Until then S.V.E.R refuses the connection and OBS only reports “Failed to connect to server”.</p>}
      <p>Viewing or replacing a key requires a sign-in confirmation from the last five minutes and a fresh authenticator or recovery code.</p>
      {account?.has_password ? <label className="field"><span>Current password {account.reauthenticated ? "(optional while recently confirmed)" : ""}</span><input type="password" autoComplete="current-password" value={password} disabled={busy || !usable} onChange={event => setPassword(event.target.value)} /></label> : <p><Link href="/account">Confirm your linked sign-in method in Account security</Link>, then return here.</p>}
      <label className="field"><span>Authenticator or recovery code</span><input autoComplete="one-time-code" maxLength={64} value={code} disabled={busy || !usable} onChange={event => setCode(event.target.value)} /></label>
      <div className="row">
        {!data.credential || data.credential.revoked ? <button disabled={busy || !usable || !code} onClick={() => keyAction("create")}>Create stream key</button> : <><button disabled={busy || !usable || !code} onClick={() => keyAction("reveal")}>Show stream key</button><button className="quiet" disabled={busy || !usable} onClick={() => setRotate(!rotate)}>Replace key…</button></>}
      </div>
      {rotate && <div className="notice"><p>Replacing this key disconnects the current publisher. Update OBS with the new key before reconnecting.</p><button className="quiet danger-text" disabled={busy || !usable || !code} onClick={() => keyAction("rotate")}>Replace key and disconnect OBS</button></div>}
      {secret && <div>
        {(secret.whip || secret.srt) && <div className="row" role="tablist" aria-label="Connection type">
          {([["rtmp", "RTMP (recommended)"], ["whip", "WHIP (lowest delay)"], ["srt", "SRT (unstable connections)"]] as const).filter(([id]) => id === "rtmp" || (id === "whip" ? secret.whip : secret.srt)).map(([id, label]) =>
            <button key={id} type="button" role="tab" aria-selected={ingest === id} className={ingest === id ? "small" : "small quiet"} onClick={() => setIngest(id)}>{label}</button>)}
        </div>}
        {ingest === "whip" && secret.whip ? <>
          <p>OBS 30 or later: in Stream settings choose Service <strong>WHIP</strong>. WHIP sends Opus audio and has fewer encoder options, so RTMP stays the default.</p>
          <label className="field"><span>Server</span><input readOnly value={secret.whip.url} /></label><button className="quiet small" onClick={() => copy(secret.whip!.url)}>Copy server</button>
          <label className="field"><span>Bearer token</span><input readOnly autoComplete="off" spellCheck={false} value={secret.whip.token} onFocus={event => event.target.select()} /></label>
          <div className="row"><button className="quiet small" onClick={() => copy(secret.whip!.token)}>Copy token</button><button className="quiet small" onClick={() => setSecret(null)}>Hide key</button></div>
        </> : ingest === "srt" && secret.srt ? <>
          <p>For mobile or travel connections that drop packets: in Stream settings choose Custom, paste this as the Server and leave Stream Key empty. It contains your key.</p>
          <label className="field"><span>Server</span><input readOnly autoComplete="off" spellCheck={false} value={secret.srt} onFocus={event => event.target.select()} /></label>
          <div className="row"><button className="quiet small" onClick={() => copy(secret.srt!)}>Copy server</button><button className="quiet small" onClick={() => setSecret(null)}>Hide key</button></div>
        </> : <>
          <label className="field"><span>Server</span><input readOnly value={secret.server} /></label><button className="quiet small" onClick={() => copy(secret.server)}>Copy server</button>
          <label className="field"><span>Stream key</span><input readOnly autoComplete="off" spellCheck={false} value={secret.key} onFocus={event => event.target.select()} /></label>
          <div className="row"><button className="quiet small" onClick={() => copy(secret.key)}>Copy key</button><button className="quiet small" onClick={() => setSecret(null)}>Hide key</button></div>
        </>}
      </div>}
    </Section>
    <Section title="OBS setup and input health" intro="Use H.264 video and AAC audio, turn B-frames off, and set a one-second keyframe interval.">
      <p>Start testing at 6 Mbps video and 160 Kbps audio, up to 1080p60. The bitrate recommendation is provisional while delivery testing continues. Step-by-step OBS setup and fixes for common problems are in <Link href="/help#obs-setup">Help</Link>.</p>
      <dl className="setup">
        <div><dt>Video / audio</dt><dd>{health.video_codec ?? "Not measured"} / {health.audio_codec ?? "Not measured"}</dd></div>
        <div><dt>Resolution</dt><dd>{health.width && health.height ? health.width + " × " + health.height : "Not measured"}</dd></div>
        <div><dt>Incoming bitrate</dt><dd>{health.input_kbps != null ? Math.round(health.input_kbps) + " Kbps" : "Not measured"}</dd></div>
        <div><dt>Keyframe interval</dt><dd>{health.keyframe_seconds != null ? (health.keyframe_seconds <= 1 ? "1 second or less" : `About ${health.keyframe_seconds} seconds`) : "Not measured"}</dd></div><div><dt>B-frames</dt><dd>{health.b_frames == null ? "Not measured" : health.b_frames ? "On" : "Off"}</dd></div><div><dt>Captions</dt><dd>{health.captions ? "Detected: viewers get a CC button" : "None detected"}</dd></div>
      </dl>
      {health.codec_warning && <p role="alert">The incoming codecs do not match H.264 and AAC. Check your OBS encoder settings.</p>}
      {health.keyframe_warning && <p role="alert">Keyframes are about {health.keyframe_seconds} seconds apart. In OBS, set Keyframe Interval to 1 s; longer intervals slow joining and recovery.</p>}
      {health.b_frames && <p role="alert">B-frames are on. In OBS, set B-frames to 0; low-latency playback can stutter with them.</p>}
      {health.bitrate_warning && <p role="alert">Incoming bitrate exceeds the provisional 8 Mbps warning level. Reduce it if playback is unstable.</p>}
      {!!broadcast?.scouts && <p className="muted">Scouted by {broadcast.scouts} {broadcast.scouts === 1 ? "viewer" : "viewers"}: they found you early and watched for 10 minutes.</p>}
      {broadcast?.observed_at && <p className="small-print">Last media observation: {new Date(broadcast.observed_at).toLocaleTimeString()}. Measurements update while OBS sends media.</p>}
    </Section>
  </>;
}

type Editor = { username: string; display_name: string };
/** Channel editors: up to 5 people who can change the title, category and language and switch Mature on. */
function Editors() {
  const [list, setList] = useState<Editor[] | null>(null);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ editors: Editor[] }>("GET", "/api/me/editors");
    if (r.ok) setList(r.data.editors); else setNote(r.error);
  }, []);
  useLoad(load);
  async function change(method: "POST" | "DELETE", username: string) {
    setNote("");
    const r = await send<{ editors: Editor[] }>(method, method === "POST" ? "/api/me/editors" : `/api/me/editors/${encodeURIComponent(username)}`, method === "POST" ? { username } : undefined);
    if (r.ok) { setList(r.data.editors); setName(""); } else setNote(r.error);
  }
  return <Section title="Editors" intro="Editors can change your title, category and language from the watch page and switch the Mature label on. They can't see your stream key, start or stop the stream, see money or change settings. Appointing needs a recent sign-in.">
    {!list ? <p className="loading">{note || "Loading…"}</p> : <ul className="list">{list.length === 0 ? <li className="muted">No editors yet.</li> : list.map(ed => <li key={ed.username} className="row between"><span>{ed.display_name} <span className="muted">@{ed.username}</span></span><button type="button" className="small quiet" onClick={() => void change("DELETE", ed.username)}>Remove</button></li>)}</ul>}
    {list && list.length < 5 && <form className="row" onSubmit={e => { e.preventDefault(); void change("POST", name.trim()); }}>
      <label className="field"><span>Username</span><input value={name} maxLength={30} onChange={e => setName(e.target.value)} /></label>
      <button className="small" disabled={!name.trim()}>Add editor</button>
    </form>}
    {note && <p role="alert" className="form-message">{note}</p>}
  </Section>;
}
