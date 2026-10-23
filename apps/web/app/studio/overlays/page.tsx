"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";
import { osOf, sceneCollection } from "../../../lib/obs";

type Kind = "follow" | "sub" | "tribute" | "skill" | "raid";
type AlertSetting = { on: boolean; text: string; seconds: number; min?: number; show_message?: boolean; tts?: boolean; image?: string | null; audio?: string | null };
type Settings = Record<Kind, AlertSetting> & { sound: "chime" | "horn" | "none"; volume: number; goal: { kind: "followers" | "subs" | null; target: number; label: string } };
type View = { settings: Settings; linked: boolean; connected: boolean };
const KINDS: [Kind, string, string][] = [
  ["follow", "Follows", "{name}"],
  ["sub", "Subscriptions", "{name}, {tier}, {months}"],
  ["tribute", "Tributes", "{name}, {valor}"],
  ["skill", "Skills", "{name}, {skill}"],
  ["raid", "Incoming raids", "{name}"],
];

/** Creator Studio → Alerts & overlays (docs/OVERLAYS.md). */
export default function OverlaysStudio() {
  const [view, setView] = useState<View | null>(null);
  const [url, setUrl] = useState("");
  const [message, setMessage] = useState("");
  const [linking, setLinking] = useState(false);
  const apply = (result: Result<View>, done: string) => { if (result.ok) { setView(result.data); setMessage(done); } else setMessage(result.error); };
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/me/overlays");
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function makeLink() {
    if (linking || (view?.linked && !window.confirm("Make a new link? The current one stops working, so update it in OBS."))) return;
    // One at a time: a second link would replace the first, which could then be the one shown.
    setLinking(true);
    const r = await send<{ url: string }>("POST", "/api/me/overlays/link");
    if (r.ok) { setUrl(r.data.url); setMessage("New link made. Copy it now: it's shown only once."); await load(); } else setMessage(r.error);
    setLinking(false);
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!view) return;
    const form = new FormData(event.currentTarget);
    const settings = structuredClone(view.settings);
    for (const [kind] of KINDS) {
      settings[kind].on = form.get(`${kind}-on`) === "on";
      settings[kind].text = String(form.get(`${kind}-text`) ?? "");
      settings[kind].seconds = Number(form.get(`${kind}-seconds`));
    }
    settings.tribute.min = Number(form.get("tribute-min"));
    settings.tribute.show_message = form.get("tribute-message") === "on";
    settings.tribute.tts = form.get("tribute-tts") === "on";
    settings.sound = form.get("sound") as Settings["sound"];
    settings.volume = Number(form.get("volume"));
    const goal = String(form.get("goal-kind") ?? "");
    settings.goal = { kind: goal ? goal as "followers" | "subs" : null, target: Number(form.get("goal-target")), label: String(form.get("goal-label") ?? "") };
    apply(await send<View>("PUT", "/api/me/overlays", settings), "Saved. Open overlays updated.");
  }
  async function media(kind: Kind, slot: "image" | "sound", file: File | null) {
    const path = `/api/me/overlays/${kind}/${slot}`;
    if (!file) { apply(await send<View>("DELETE", path), slot === "image" ? "Image removed." : "Sound removed."); return; }
    const form = new FormData();
    form.append("file", file);
    setMessage("Uploading…");
    apply(await send<View>("POST", path, form), slot === "image" ? "Image saved." : "Sound saved.");
  }
  async function download(alerts: string) {
    const me = await send<{ username: string }>("GET", "/api/auth/me");
    if (!me.ok) { setMessage(me.error); return; }
    const collection = sceneCollection({ alerts, chat: `${location.origin}/${me.data.username}/chat?overlay=1`, os: osOf(navigator.userAgent) });
    const link = document.createElement("a");
    link.href = URL.createObjectURL(new Blob([JSON.stringify(collection, null, 2)], { type: "application/json" }));
    link.download = "SVER.json";
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  }
  async function test(kind: Kind) {
    const r = await send("POST", "/api/me/overlays/test", { kind });
    setMessage(r.ok ? "Test sent to open overlays." : r.error);
  }
  if (!view) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  const s = view.settings;
  return <><h1>Alerts &amp; overlays</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    <Section title="Browser source" intro="Add the link to OBS as a Browser source (1920 × 1080 works well). It's private: anyone with it sees your alerts, so make a new one if it leaks.">
      {url ? <ul className="list">
        <li><strong>Alert box:</strong> <code>{url}</code></li>
        <li><strong>Goal bar:</strong> <code>{`${url}?widget=goal`}</code></li>
        <li><strong>Recent events:</strong> <code>{`${url}?widget=events`}</code></li>
        <li><strong>New to OBS?</strong> <button type="button" className="small quiet" onClick={() => void download(url)}>Download an OBS scene collection</button> with these, your chat and (on Windows) Game Capture already in place, for a 1920 × 1080 canvas. In OBS choose Scene Collection → Import. It holds this link, so keep the file private.</li>
        <li><strong>StreamElements or Streamlabs:</strong> add a Custom widget (StreamElements) or Custom Widget (Streamlabs) and paste this as its HTML, with empty CSS and JS: <code>{`<iframe src="${url}" style="border:0;width:100%;height:100%" allow="autoplay"></iframe>`}</code></li>
      </ul> : <p className="muted">{view.linked ? `A link exists${view.connected ? " and an overlay is open now" : ""}. Make a new one to see it again.` : "No link yet."}</p>}
      <div className="row wrap">
        <button type="button" className="small" disabled={linking} onClick={makeLink}>{view.linked ? "Make a new link" : "Make a link"}</button>
        {view.linked && <button type="button" className="small quiet danger-text" onClick={async () => apply(await send<View>("DELETE", "/api/me/overlays/link"), "Link turned off.")}>Turn off the link</button>}
      </div>
    </Section>
    <Section title="Alerts" intro="Messages can use the names shown beside each type. Alerts play one at a time, in order.">
      <form onSubmit={save} className="stack">
        {KINDS.map(([kind, label, names]) => <fieldset key={kind} className="stack">
          <legend>{label}</legend>
          <label className="row"><input type="checkbox" name={`${kind}-on`} defaultChecked={s[kind].on} /> Show</label>
          <label className="field"><span>Message ({names})</span><input name={`${kind}-text`} defaultValue={s[kind].text} maxLength={120} required /></label>
          <label className="field narrow"><span>Seconds on screen (3–15)</span><input name={`${kind}-seconds`} type="number" min={3} max={15} defaultValue={s[kind].seconds} /></label>
          {kind === "tribute" && <>
            <label className="field narrow"><span>Only tributes of at least (Valor)</span><input name="tribute-min" type="number" min={10} defaultValue={s.tribute.min} /></label>
            <label className="row"><input type="checkbox" name="tribute-message" defaultChecked={s.tribute.show_message} /> Show the tribute&apos;s message</label>
            <label className="row"><input type="checkbox" name="tribute-tts" defaultChecked={s.tribute.tts ?? false} /> Read the message aloud (text-to-speech with your PC&apos;s voices, at the overlay volume)</label>
          </>}
          <div className="row wrap">
            <label className="field narrow"><span>Image (PNG, JPEG or WebP, up to 2 MB){s[kind].image ? " · set" : ""}</span><input type="file" accept="image/png,image/jpeg,image/webp" onChange={e => { const f = e.currentTarget.files?.[0]; e.currentTarget.value = ""; if (f) void media(kind, "image", f); }} /></label>
            {s[kind].image && <button type="button" className="small quiet" onClick={() => void media(kind, "image", null)}>Remove image</button>}
            <label className="field narrow"><span>Sound (MP3, OGG or WAV, first 10 seconds, up to 1 MB){s[kind].audio ? " · set" : ""}</span><input type="file" accept="audio/mpeg,audio/ogg,audio/wav,audio/x-wav" onChange={e => { const f = e.currentTarget.files?.[0]; e.currentTarget.value = ""; if (f) void media(kind, "sound", f); }} /></label>
            {s[kind].audio && <button type="button" className="small quiet" onClick={() => void media(kind, "sound", null)}>Remove sound</button>}
          </div>
          <button type="button" className="small quiet" onClick={() => void test(kind)}>Send a test</button>
        </fieldset>)}
        <label className="field narrow"><span>Sound when an alert has no uploaded sound</span><select name="sound" defaultValue={s.sound}><option value="chime">Chime</option><option value="horn">Horn</option><option value="none">None</option></select></label>
        <label className="field narrow"><span>Volume (0–100)</span><input name="volume" type="number" min={0} max={100} defaultValue={s.volume} /></label>
        <fieldset className="stack"><legend>Goal bar</legend>
          <label className="field narrow"><span>Count</span><select name="goal-kind" defaultValue={s.goal.kind ?? ""}><option value="">No goal</option><option value="followers">Followers</option><option value="subs">Active subscribers</option></select></label>
          <label className="field narrow"><span>Target</span><input name="goal-target" type="number" min={1} defaultValue={s.goal.target} /></label>
          <label className="field"><span>Label (up to 40 characters)</span><input name="goal-label" maxLength={40} defaultValue={s.goal.label} /></label>
        </fieldset>
        <button type="submit" className="small">Save</button>
      </form>
    </Section>
  </>;
}
