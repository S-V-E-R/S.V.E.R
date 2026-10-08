"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Preferences = { skip_mature_warning: boolean; adult: boolean };

/** Settings -> Preferences (docs/CHANNEL_ADDITIONS.md "Mature label"). */
export default function PreferencesPage() {
  const [prefs, setPrefs] = useState<Preferences | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await send<Preferences>("GET", "/api/me/preferences");
    if (result.ok) setPrefs(result.data); else setMessage(result.error);
  }, []);
  useLoad(load);
  async function save(skip: boolean) {
    const result = await send<Preferences>("PUT", "/api/me/preferences", { skip_mature_warning: skip });
    if (result.ok) { setPrefs(result.data); setMessage("Saved."); } else setMessage(result.error);
  }
  return <><h1>Preferences</h1>
    {!prefs ? <p className="loading">{message || "Loading…"}</p> : <section className="panel">
      <label className="checkbox"><input type="checkbox" checked={prefs.skip_mature_warning} disabled={!prefs.adult} onChange={event => void save(event.target.checked)} /> Don&apos;t warn me about mature streams</label>
      <p className="muted small">{prefs.adult ? "Streams labeled mature play without the warning screen first." : "Streams labeled mature aren't available on accounts under 18."}</p>
      {message && <p role="status" className="form-message">{message}</p>}
    </section>}
  </>;
}
