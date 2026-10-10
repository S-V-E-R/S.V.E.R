"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Style = { key: string; name: string; level: number; unlocked: boolean };
type Earned = { key: string; name: string; description: string };
type View = { level: number; name_style: string; frame: number | null; max_frame: number; showcase: string[]; styles: Style[]; earned: Earned[] };

/** Settings → Cosmetics (docs/ACHIEVEMENTS.md): only what your level and achievements unlocked. */
export default function Cosmetics() {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/me/cosmetics");
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const frame = String(form.get("frame") ?? "");
    const r = await send<View>("PUT", "/api/me/cosmetics", { name_style: form.get("style"), frame: frame === "" ? null : Number(frame), showcase: form.getAll("showcase") });
    if (r.ok) { setView(r.data); setMessage("Saved."); } else setMessage(r.error);
  }
  if (!view) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <section className="panel section"><h1>Cosmetics</h1>
    <p className="muted">Level {view.level}. Styles and frames unlock with levels; the showcase holds up to 3 of your achievements. None of this affects MAGNet, factions or money.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    <form onSubmit={save} className="stack">
      <fieldset className="stack"><legend>Name style in chat</legend>
        {view.styles.map(s => <label key={s.key} className="row"><input type="radio" name="style" value={s.key} defaultChecked={view.name_style === s.key} disabled={!s.unlocked} /> <span className="faction-name" data-style={s.key}>{s.name}</span> {!s.unlocked && <span className="muted small">(level {s.level})</span>}</label>)}
      </fieldset>
      <label className="field narrow"><span>Badge frame</span><select name="frame" defaultValue={view.frame ?? ""}>
        <option value="">Newest unlocked</option>
        {Array.from({ length: view.max_frame + 1 }, (_, f) => <option key={f} value={f}>{f === 0 ? "No frame" : `Level ${f * 10} frame`}</option>)}
      </select></label>
      <fieldset className="stack"><legend>Showcase (up to 3)</legend>
        {view.earned.length ? view.earned.map(a => <label key={a.key} className="row"><input type="checkbox" name="showcase" value={a.key} defaultChecked={view.showcase.includes(a.key)} /> <strong>{a.name}</strong> <span className="muted small">{a.description}</span></label>)
          : <p className="muted">No achievements yet: they come from following, chatting, scouting, daily orders, levels and streaming.</p>}
      </fieldset>
      <button type="submit" className="small">Save</button>
    </form>
  </section>;
}
