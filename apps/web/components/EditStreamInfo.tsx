"use client";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { LANGUAGES } from "../lib/languages";
import { GamePicker } from "./GamePicker";

type Info = { title: string; category_id: string | null; revision: number; mature: boolean; language: string | null };

/**
 * "Edit stream info" on the watch page for the channel's editors (docs/CHANNEL_ADDITIONS.md
 * "Channel editors"): title, category, language, and switching the Mature label on. Hidden for
 * everyone else (the API answers 404).
 */
export function EditStreamInfo({ username }: { username: string }) {
  const path = `/api/channels/${encodeURIComponent(username)}/stream`;
  const [info, setInfo] = useState<Info | null>(null);
  const [owner, setOwner] = useState(false);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await send<{ settings: Info | null; owner: boolean }>("GET", path);
    if (r.ok && r.data.settings) { setInfo(r.data.settings); setOwner(r.data.owner); }
  }, [path]);
  useLoad(load);
  if (!info || owner) return null;
  async function save(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!info) return;
    setBusy(true); setNote("");
    const r = await send<{ revision: number }>("PATCH", path, info);
    setBusy(false);
    if (r.ok) { setInfo({ ...info, revision: r.data.revision }); setNote("Saved. The streamer sees that you made this change."); }
    else { setNote(r.error); if (r.status === 409) void load(); }
  }
  return <details className="panel edit-stream-info">
    <summary>Edit stream info</summary>
    <form onSubmit={save}>
      <label className="field"><span>Title</span><input required maxLength={140} value={info.title} onChange={e => setInfo({ ...info, title: e.target.value })} /></label>
      <GamePicker value={info.category_id ?? ""} disabled={busy} onChange={(category_id, mature) => setInfo({ ...info, category_id, mature: info.mature || mature })} />
      <label className="field narrow"><span>Language</span><select value={info.language ?? "other"} onChange={e => setInfo({ ...info, language: e.target.value })}>
        {LANGUAGES.map(([code, name]) => <option key={code} value={code}>{name}</option>)}
      </select></label>
      <label className="checkbox"><input type="checkbox" checked={info.mature} disabled={info.mature} onChange={e => setInfo({ ...info, mature: e.target.checked })} /> Mature{info.mature && " (only the streamer can turn this off)"}</label>
      <button className="small" disabled={busy}>Save</button>
      {note && <p role="status" className="form-message">{note}</p>}
    </form>
  </details>;
}
