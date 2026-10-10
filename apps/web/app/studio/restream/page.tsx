"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Destination = { id: string; platform: string; label: string; server: string; enabled: boolean; status: string; detail: string | null };
type Restream = { destinations: Destination[]; max: number; available: boolean };

const PLATFORMS: Record<string, string> = { twitch: "Twitch", youtube: "YouTube", kick: "Kick", custom: "Custom RTMP" };
const STATUS: Record<string, string> = { idle: "Off air", starting: "Connecting…", live: "Live", reconnecting: "Reconnecting", rejected: "Key rejected", waiting: "Waiting for capacity" };

/** Creator Studio → Restream (docs/LINKED_CHAT.md "Restreaming"). */
export default function RestreamPage() {
  const [data, setData] = useState<Restream | null>(null);
  const [message, setMessage] = useState("");
  const [platform, setPlatform] = useState("twitch");
  const apply = (result: Result<Restream>, saved: string) => {
    if (result.ok) { setData(result.data); setMessage(saved); } else setMessage(result.error);
  };
  const load = useCallback(async () => {
    const result = await send<Restream>("GET", "/api/me/restream");
    if (result.ok) setData(result.data); else setMessage(result.error);
  }, []);
  useLoad(load);
  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const values = Object.fromEntries(new FormData(form)) as Record<string, string>;
    const result = await send<Restream>("POST", "/api/me/restream", { platform, server: values.server || null, key: values.key, label: values.label });
    apply(result, `${PLATFORMS[platform]} added. It goes live the next time you do.`);
    if (result.ok) form.reset();
  }
  async function replaceKey(event: FormEvent<HTMLFormElement>, d: Destination) {
    event.preventDefault();
    const form = event.currentTarget;
    const key = String(new FormData(form).get("key") ?? "");
    const result = await send<Restream>("PATCH", `/api/me/restream/${d.id}`, { key });
    apply(result, "Stream key replaced.");
    if (result.ok) form.reset();
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>Restream</h1>
    <Section title="Send your stream to other platforms" intro="Keep streaming to S.V.E.R from OBS as usual. While you're live, S.V.E.R relays the same stream to up to three other platforms, unchanged and without re-encoding, so they get the quality you send. If a platform drops, S.V.E.R reconnects; your S.V.E.R stream is never affected.">
      {!data.available && <p className="muted">Restreaming isn&apos;t switched on for this site yet. You can add destinations now; they start once it is.</p>}
      {message && <p role="status" className="form-message">{message}</p>}
      {data.destinations.length > 0 && <ul className="list">{data.destinations.map(d => <li key={d.id} className="stack">
        <div className="row between wrap">
          <span><strong>{d.label || PLATFORMS[d.platform]}</strong> <span className="muted small">{d.server}</span></span>
          <span className={`badge${d.status === "live" ? " verified" : ""}`} title={d.detail ?? undefined}>{STATUS[d.status] ?? d.status}</span>
        </div>
        {d.detail && d.status !== "idle" && <p className="muted small">{d.detail}</p>}
        <div className="row wrap">
          <label className="row"><input type="checkbox" checked={d.enabled} onChange={async e => apply(await send<Restream>("PATCH", `/api/me/restream/${d.id}`, { enabled: e.target.checked }), e.target.checked ? "Switched on." : "Switched off.")} /> Send my stream here</label>
          <form className="row" onSubmit={e => replaceKey(e, d)}><input name="key" type="password" autoComplete="off" placeholder="New stream key" aria-label={`New stream key for ${PLATFORMS[d.platform]}`} required /><button type="submit" className="small quiet">Replace key</button></form>
          <button type="button" className="small quiet danger-text" onClick={async () => apply(await send<Restream>("DELETE", `/api/me/restream/${d.id}`), "Destination deleted, with its key.")}>Delete</button>
        </div>
      </li>)}</ul>}
      {data.destinations.length < data.max ? <form className="stack" onSubmit={add}>
        <h3>Add a destination</h3>
        <label className="field narrow"><span>Platform</span><select value={platform} onChange={e => setPlatform(e.target.value)}>{Object.entries(PLATFORMS).map(([v, n]) => <option key={v} value={v}>{n}</option>)}</select></label>
        <label className="field"><span>Server{platform === "twitch" || platform === "youtube" ? " (optional; the platform's default is used)" : ""}</span><input name="server" placeholder={platform === "twitch" ? "rtmp://live.twitch.tv/app" : platform === "youtube" ? "rtmp://a.rtmp.youtube.com/live2" : "rtmps://…"} required={platform === "kick" || platform === "custom"} autoComplete="off" /></label>
        <label className="field"><span>Stream key</span><input name="key" type="password" autoComplete="off" required /><small>From the platform&apos;s stream settings. S.V.E.R stores it encrypted and never shows it again; you can replace it any time.</small></label>
        <label className="field narrow"><span>Name (optional)</span><input name="label" maxLength={40} /></label>
        <button type="submit">Add destination</button>
      </form> : <p className="muted">You&apos;re sending to the most destinations (three). Delete one to add another.</p>}
      <p className="muted small">Some platforms don&apos;t allow other platforms&apos; chat inside the video you send them, so keep S.V.E.R chat off the video you restream.</p>
    </Section>
  </>;
}
