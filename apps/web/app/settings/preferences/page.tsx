"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";
import { LANGUAGES, fromBrowser } from "../../../lib/languages";
import { ADS_OFF_WARNING } from "../../../components/Ads";

type Preferences = { skip_mature_warning: boolean; adult: boolean; languages: string[]; only_my_languages: boolean; ads_off: boolean };

/** Settings -> Preferences (docs/CHANNEL_ADDITIONS.md "Mature label"). */
export default function PreferencesPage() {
  const [prefs, setPrefs] = useState<Preferences | null>(null);
  const [message, setMessage] = useState("");
  const [enlisted, setEnlisted] = useState<number | null>(null);
  const load = useCallback(async () => {
    const [result, referrals] = await Promise.all([send<Preferences>("GET", "/api/me/preferences"), send<{ enlisted: number }>("GET", "/api/auth/referrals")]);
    if (result.ok) setPrefs(result.data); else setMessage(result.error);
    if (referrals.ok) setEnlisted(referrals.data.enlisted);
  }, []);
  useLoad(load);
  async function save(change: Partial<Preferences>) {
    const result = await send<Preferences>("PUT", "/api/me/preferences", change);
    if (result.ok) { setPrefs(result.data); setMessage("Saved."); } else setMessage(result.error);
  }
  return <><h1>Preferences</h1>
    {!prefs ? <p className="loading">{message || "Loading…"}</p> : <section className="panel">
      <label className="checkbox"><input type="checkbox" checked={prefs.skip_mature_warning} disabled={!prefs.adult} onChange={event => void save({ skip_mature_warning: event.target.checked })} /> Don&apos;t warn me about mature streams</label>
      <p className="muted small">{prefs.adult ? "Streams labeled mature play without the warning screen first." : "Streams labeled mature aren't available on accounts under 18."}</p>
    </section>}
    {prefs && <LanguagePrefs prefs={prefs} save={save} />}
    {prefs && <section className="panel">
      <h2>Ads</h2>
      <p className="muted small">Ads on S.V.E.R never come before a stream and never cover it: during a break the stream keeps playing beside the ad, muted with captions on. While they&apos;re on, you earn Ad Valor, a share of every ad you watch.</p>
      <label className="checkbox"><input type="checkbox" checked={!prefs.ads_off} onChange={event => { if (event.target.checked || window.confirm(ADS_OFF_WARNING)) void save({ ads_off: !event.target.checked }); }} /> Show me ads</label>
      {prefs.ads_off && <p className="muted small">Ads are off. You don&apos;t see any and don&apos;t earn Ad Valor.</p>}
    </section>}
    {enlisted !== null && <section className="panel"><h2>Links you shared</h2><p>{enlisted === 1 ? "1 person enlisted from a link with your username." : `${enlisted.toLocaleString()} people enlisted from a link with your username.`}</p><p className="muted small">That number is only for you. It does not change who gets seen.</p></section>}
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}

/** Languages I watch in (defaults to the browser's) and the home filter. */
function LanguagePrefs({ prefs, save }: { prefs: Preferences; save: (change: Partial<Preferences>) => Promise<void> }) {
  const chosen = prefs.languages.length ? prefs.languages : fromBrowser(navigator.languages);
  const toggle = (code: string, on: boolean) => void save({ languages: on ? [...chosen, code].slice(0, 10) : chosen.filter(c => c !== code) });
  return <section className="panel">
    <h2>Languages I watch in</h2>
    <p className="muted small">Streams in other languages show their language on the card. {prefs.languages.length ? "" : "Until you choose, your browser's languages are used."}</p>
    <div className="row wrap">{LANGUAGES.filter(([code]) => code !== "other").map(([code, name]) =>
      <label key={code} className="checkbox"><input type="checkbox" checked={chosen.includes(code)} disabled={!chosen.includes(code) && chosen.length >= 10} onChange={event => toggle(code, event.target.checked)} /> {name}</label>)}</div>
    <label className="checkbox"><input type="checkbox" checked={prefs.only_my_languages} onChange={event => void save({ only_my_languages: event.target.checked, languages: chosen })} /> Only show streams in my languages on the homepage</label>
  </section>;
}
