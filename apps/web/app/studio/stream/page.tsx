"use client";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send } from "../../../lib/client-api";

type Settings = { title: string; category_id: string | null; revision: number };
type Health = { video_codec?: string | null; audio_codec?: string | null; width?: number | null; height?: number | null; input_kbps?: number | null; codec_warning?: boolean; bitrate_warning?: boolean };
type Stream = {
  configured: boolean; eligible: boolean; settings: Settings; disconnect_pending: boolean;
  last_broadcast?: { ended_at: string; not_counted: number } | null;
  credential: { created_at: string; revoked: boolean } | null;
  broadcast: { state: string; started_at: string; reconnect_deadline: string | null; observed_at: string | null; end_reason?: string | null; health: Health } | null;
};
type Account = { has_password: boolean; reauthenticated: boolean };
type Key = { server: string; key: string; disconnect_pending: boolean };

export default function StreamStudio() {
  const [data, setData] = useState<Stream | null>(null);
  // When data last arrived; staleness is judged against it so render stays pure.
  const [seenAt, setSeenAt] = useState(0);
  const [form, setForm] = useState<Settings | null>(null);
  const [categories, setCategories] = useState<{ id: string; name: string }[]>([]);
  const [account, setAccount] = useState<Account | null>(null);
  const [status, setStatus] = useState<SaveState>({});
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [secret, setSecret] = useState<Key | null>(null);
  const [rotate, setRotate] = useState(false);
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
    Promise.all([send<{ categories: { id: string; name: string }[] }>("GET", "/api/categories"), send<Account>("GET", "/api/auth/me")]).then(([catalog, me]) => {
      if (!active.current) return;
      if (catalog.ok) setCategories(catalog.data.categories);
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
    const result = await send<{ revision: number }>("PATCH", "/api/me/stream", form);
    if (result.ok) {
      setForm(previous => previous && { ...previous, revision: result.data.revision });
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
        <label className="field"><span>Category</span><select required value={form.category_id ?? ""} disabled={busy || !data.eligible} onChange={event => setForm({ ...form, category_id: event.target.value })}><option value="">Choose a category</option>{categories.map(category => <option key={category.id} value={category.id}>{category.name}</option>)}</select></label>
        <div className="row"><button disabled={busy || !data.eligible || count < 1 || count > 140}>Save details</button><button type="button" className="quiet" disabled={busy} onClick={reloadDetails}>Reload saved details</button></div>
      </form>
    </Section>
    <Section title="OBS connection" intro="Use Custom in OBS's Stream settings. Keep your stream key private.">
      <p>Viewing or replacing a key requires a sign-in confirmation from the last five minutes and a fresh authenticator or recovery code.</p>
      {account?.has_password ? <label className="field"><span>Current password {account.reauthenticated ? "(optional while recently confirmed)" : ""}</span><input type="password" autoComplete="current-password" value={password} disabled={busy || !usable} onChange={event => setPassword(event.target.value)} /></label> : <p><Link href="/account">Confirm your linked sign-in method in Account security</Link>, then return here.</p>}
      <label className="field"><span>Authenticator or recovery code</span><input autoComplete="one-time-code" maxLength={64} value={code} disabled={busy || !usable} onChange={event => setCode(event.target.value)} /></label>
      <div className="row">
        {!data.credential || data.credential.revoked ? <button disabled={busy || !usable || !code} onClick={() => keyAction("create")}>Create stream key</button> : <><button disabled={busy || !usable || !code} onClick={() => keyAction("reveal")}>Show stream key</button><button className="quiet" disabled={busy || !usable} onClick={() => setRotate(!rotate)}>Replace key…</button></>}
      </div>
      {rotate && <div className="notice"><p>Replacing this key disconnects the current publisher. Update OBS with the new key before reconnecting.</p><button className="quiet danger-text" disabled={busy || !usable || !code} onClick={() => keyAction("rotate")}>Replace key and disconnect OBS</button></div>}
      {secret && <div>
        <label className="field"><span>Server</span><input readOnly value={secret.server} /></label><button className="quiet small" onClick={() => copy(secret.server)}>Copy server</button>
        <label className="field"><span>Stream key</span><input readOnly autoComplete="off" spellCheck={false} value={secret.key} onFocus={event => event.target.select()} /></label>
        <div className="row"><button className="quiet small" onClick={() => copy(secret.key)}>Copy key</button><button className="quiet small" onClick={() => setSecret(null)}>Hide key</button></div>
      </div>}
    </Section>
    <Section title="OBS setup and input health" intro="Use H.264 video and AAC audio, turn B-frames off, and set a one-second keyframe interval.">
      <p>Start testing at 6 Mbps video and 160 Kbps audio, up to 1080p60. The bitrate recommendation is provisional while delivery testing continues.</p>
      <dl className="setup">
        <div><dt>Video / audio</dt><dd>{health.video_codec ?? "Not measured"} / {health.audio_codec ?? "Not measured"}</dd></div>
        <div><dt>Resolution</dt><dd>{health.width && health.height ? health.width + " × " + health.height : "Not measured"}</dd></div>
        <div><dt>Incoming bitrate</dt><dd>{health.input_kbps != null ? Math.round(health.input_kbps) + " Kbps" : "Not measured"}</dd></div>
        <div><dt>Keyframe interval</dt><dd>Not measured</dd></div><div><dt>B-frames</dt><dd>Not measured</dd></div>
      </dl>
      {health.codec_warning && <p role="alert">The incoming codecs do not match H.264 and AAC. Check your OBS encoder settings.</p>}
      {health.bitrate_warning && <p role="alert">Incoming bitrate exceeds the provisional 8 Mbps warning level. Reduce it if playback is unstable.</p>}
      {broadcast?.observed_at && <p className="small-print">Last media observation: {new Date(broadcast.observed_at).toLocaleTimeString()}. Measurements update while OBS sends media.</p>}
    </Section>
  </>;
}
