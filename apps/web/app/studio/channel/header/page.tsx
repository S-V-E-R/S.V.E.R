"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, STALE, Status, type SaveState } from "../../../../components/Form";
import { send, useLoad } from "../../../../lib/client-api";

type Header = { page_label: string; welcome_line: string; intro_title: string; intro_body: string; page_vibe: string; enabled: boolean; defaults: { page_label: string; welcome_line: string }; revision: number };

/** Owner-editable decorative header copy (docs/PROFILES.md, P9). Plain text only. */
export default function HeaderStudio() {
  const [data, setData] = useState<Header | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<Header>("GET", "/api/me/header"); if (r.ok) setData(r.data); }, []);
  useLoad(load);
  if (!data) return <p className="loading">Loading…</p>;
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const f = new FormData(event.currentTarget);
    const r = await send("PUT", "/api/me/header", { page_label: f.get("page_label"), welcome_line: f.get("welcome_line"), intro_title: f.get("intro_title"), intro_body: f.get("intro_body"), page_vibe: f.get("page_vibe"), enabled: f.get("enabled") === "on", revision: data!.revision });
    setState(r.ok ? { saved: "Page header saved." } : r.status === 409 ? { error: STALE } : r);
    if (r.ok) load();
  }
  const err = (f: string) => state.field === f ? state.error : undefined;
  return <><h1>Page header</h1>
    <form onSubmit={save} noValidate key={data.revision}>
      <Section title="Label and welcome" intro="Shown above and below your name. Leave a field empty to use the default.">
        <label className="checkbox"><input type="checkbox" name="enabled" defaultChecked={data.enabled} /> Show the label and welcome line</label>
        <label className="field"><span>Page label</span><input name="page_label" defaultValue={data.page_label} maxLength={24} placeholder={data.defaults.page_label} aria-invalid={!!err("page_label")} /><small>Up to 24 characters. Default: {data.defaults.page_label}.</small></label>
        <label className="field"><span>Welcome line</span><input name="welcome_line" defaultValue={data.welcome_line} maxLength={80} placeholder={data.defaults.welcome_line} aria-invalid={!!err("welcome_line")} /><small>Up to 80 characters. Default: {data.defaults.welcome_line}.</small></label>
      </Section>
      <Section title="Intro card" intro="A short card at the top of your Home tab. It shows only when the text isn't empty.">
        <label className="field"><span>Title</span><input name="intro_title" defaultValue={data.intro_title} maxLength={60} placeholder="About this page" aria-invalid={!!err("intro_title")} /><small>Up to 60 characters.</small></label>
        <label className="field"><span>Text</span><textarea name="intro_body" defaultValue={data.intro_body} maxLength={500} rows={4} aria-invalid={!!err("intro_body")} /><small>Up to 500 characters and 6 line breaks. Plain text.</small></label>
      </Section>
      <Section title="Page vibe" intro="A few words next to your join date, like “Chill strategy nights”.">
        <label className="field"><span>Vibe</span><input name="page_vibe" defaultValue={data.page_vibe} maxLength={24} aria-invalid={!!err("page_vibe")} /><small>Up to 24 characters. Leave empty to hide.</small></label>
      </Section>
      <div className="row"><button type="submit" className="small">Save page header</button></div>
      <Status state={state} />
    </form>
  </>;
}
