"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import type { BoardDef, Control } from "../../../components/Board";
import { EffectStage, useShown, type BoardEvent } from "../../../components/BoardEffects";
import { CopyButton } from "../../../components/CopyButton";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Mine = {
  username: string; draft: BoardDef; published: BoardDef | null; version: number; published_at: string | null;
  disabled: boolean; moderators_run: boolean; overlay: { set: boolean; connected: boolean }; webhook_url: string | null;
  checklist: { label: string; ok: boolean; required: boolean }[]; templates: { name: string; board: BoardDef }[];
  effects: string[]; kinds: Control["kind"][]; audiences: Control["audience"][]; webhook_secret?: string | null;
};
const KIND_NAMES: Record<Control["kind"], string> = { button: "Button", label: "Label", text: "Text input", goal: "Goal", joystick: "Joystick", rally: "Faction rally" };
const AUDIENCE_NAMES: Record<Control["audience"], string> = { everyone: "Everyone signed in", followers: "Followers", subscribers: "Subscribers", moderators: "Moderators" };
const num = (value: string) => (value === "" ? 0 : Number(value));

function fresh(kind: Control["kind"], taken: Set<string>): Control {
  let n = 1;
  while (taken.has(`${kind}-${n}`)) n++;
  return { id: `${kind}-${n}`, kind, label: KIND_NAMES[kind], cost: 0, cooldown_seconds: 0, per_stream_limit: null, audience: "everyone", effect: "none", target: kind === "goal" ? 100 : null, width: kind === "label" ? 4 : 1 };
}

function ControlEditor({ control, mine, onChange, onMove, onRemove, onTest, saved }: { control: Control; mine: Mine; onChange: (c: Control) => void; onMove: (by: number) => void; onRemove: () => void; onTest: () => void; saved: boolean }) {
  const c = control;
  const simple = c.kind === "label" || c.kind === "joystick" || c.kind === "rally";
  return <fieldset>
    <legend>{KIND_NAMES[c.kind]} · <code>{c.id}</code></legend>
    <label className="field"><span>Label</span><input value={c.label} maxLength={40} required onChange={e => onChange({ ...c, label: e.target.value })} /></label>
    {!simple && <>
      <label className="field narrow"><span>Cost (Engagement Valor, 0 is free)</span><input type="number" min={0} max={100000} value={c.cost} onChange={e => onChange({ ...c, cost: num(e.target.value) })} /></label>
      <label className="field narrow"><span>Cooldown per viewer (seconds)</span><input type="number" min={0} max={3600} value={c.cooldown_seconds} onChange={e => onChange({ ...c, cooldown_seconds: num(e.target.value) })} /></label>
      <label className="field narrow"><span>Limit per stream</span><input type="number" min={1} max={1000} placeholder="No limit" value={c.per_stream_limit ?? ""} onChange={e => onChange({ ...c, per_stream_limit: e.target.value ? Number(e.target.value) : null })} /></label>
      <label className="field narrow"><span>Effect</span><select value={c.effect} onChange={e => onChange({ ...c, effect: e.target.value })}>{mine.effects.map(fx => <option key={fx} value={fx}>{fx === "none" ? "None" : fx[0].toUpperCase() + fx.slice(1)}</option>)}</select></label>
    </>}
    {c.kind !== "label" && <label className="field narrow"><span>Who can use it</span><select value={c.audience} onChange={e => onChange({ ...c, audience: e.target.value as Control["audience"] })}>{mine.audiences.map(a => <option key={a} value={a}>{AUDIENCE_NAMES[a]}</option>)}</select></label>}
    {c.kind === "goal" && <label className="field narrow"><span>Goal</span><input type="number" min={1} max={1000000} value={c.target ?? 1} onChange={e => onChange({ ...c, target: num(e.target.value) })} /></label>}
    <label className="field narrow"><span>Width</span><select value={c.width} onChange={e => onChange({ ...c, width: Number(e.target.value) })}>{[1, 2, 3, 4].map(w => <option key={w} value={w}>{w} of 4</option>)}</select></label>
    <div className="row wrap">
      <button type="button" className="small quiet" onClick={() => onMove(-1)} aria-label={`Move ${c.label} up`}>↑</button>
      <button type="button" className="small quiet" onClick={() => onMove(1)} aria-label={`Move ${c.label} down`}>↓</button>
      {c.kind !== "label" && c.kind !== "joystick" && <button type="button" className="small quiet" disabled={!saved} title={saved ? undefined : "Save the draft first"} onClick={onTest}>Test</button>}
      <button type="button" className="small quiet" onClick={onRemove}>Remove</button>
    </div>
  </fieldset>;
}

/** Creator Studio → Board: build, test and publish the channel's CrowdSync board (docs/CROWDSYNC.md "Boards"). */
export default function StudioBoard() {
  const [mine, setMine] = useState<Mine | null>(null);
  const [board, setBoard] = useState<BoardDef | null>(null);
  const [screen, setScreen] = useState(0);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const [webhook, setWebhook] = useState("");
  const [secret, setSecret] = useState<string | null>(null);
  const [overlayUrl, setOverlayUrl] = useState<string | null>(null);
  const [shown, add] = useShown();
  const accept = useCallback((data: Mine, keepDraft = false) => {
    setMine(data);
    if (!keepDraft) { setBoard(data.draft); setDirty(false); }
    setWebhook(data.webhook_url ?? "");
  }, []);
  const load = useCallback(async () => {
    const r = await send<Mine>("GET", "/api/me/board");
    if (r.ok) accept(r.data); else setState({ error: r.error });
  }, [accept]);
  useLoad(load);
  if (!mine || !board) return state.error ? <p role="alert" className="form-message error">{state.error}</p> : <p className="loading">Loading…</p>;

  const current = board.screens[Math.min(screen, board.screens.length - 1)];
  const taken = new Set(board.screens.flatMap(s => s.controls.map(c => c.id)));
  function edit(next: BoardDef) { setBoard(next); setDirty(true); setState({}); }
  function editScreen(change: (controls: Control[]) => Control[], name?: string) {
    edit({ screens: board!.screens.map((s, i) => i === screen ? { name: name ?? s.name, controls: change(s.controls) } : s) });
  }
  async function act(method: string, path: string, body?: unknown, done?: string, keepDraft = false) {
    setBusy(true); setState({});
    const r = await send<Mine>(method, path, body);
    setBusy(false);
    if (!r.ok) { setState({ error: r.error }); return null; }
    accept(r.data, keepDraft);
    if (done) setState({ saved: done });
    return r.data;
  }
  async function test(control: Control) {
    const r = await send<{ effect: BoardEvent }>("POST", "/api/me/board/test", { control: control.id });
    if (r.ok) add(r.data.effect); else setState({ error: r.error });
  }
  const required = mine.checklist.filter(c => c.required && !c.ok);
  const live = `/api/channels/${encodeURIComponent(mine.username)}/board`;

  return <><h1>Board</h1>
    <p className="intro">Viewers use your board under the player. Presses spend your channel&apos;s Engagement Valor (or are free) and play effects on stream. <Link href={`/${mine.username}/live`}>See it on your channel</Link>.</p>
    <Section title="Build" intro="Edits change only the draft. Viewers keep the published board until you publish again.">
      <div className="row wrap">
        <label className="field narrow"><span>Start from a template</span><select value="" onChange={e => { const t = mine.templates.find(x => x.name === e.target.value); if (t && window.confirm(`Replace the draft with "${t.name}"?`)) { edit(t.board); setScreen(0); } }}><option value="">Choose…</option>{mine.templates.map(t => <option key={t.name}>{t.name}</option>)}</select></label>
      </div>
      <div className="board-screens" role="tablist">{board.screens.map((s, i) => <button key={i} type="button" role="tab" aria-selected={i === screen} className={i === screen ? "small" : "small quiet"} onClick={() => setScreen(i)}>{s.name || "Untitled"}</button>)}
        {board.screens.length < 4 && <button type="button" className="small quiet" onClick={() => { edit({ screens: [...board.screens, { name: `Screen ${board.screens.length + 1}`, controls: [] }] }); setScreen(board.screens.length); }}>+ Screen</button>}
      </div>
      <div className="board-builder">
        <div className="row wrap">
          <label className="field narrow"><span>Screen name</span><input value={current.name} maxLength={30} onChange={e => editScreen(c => c, e.target.value)} /></label>
          {board.screens.length > 1 && <button type="button" className="small quiet" onClick={() => { edit({ screens: board.screens.filter((_, i) => i !== screen) }); setScreen(0); }}>Remove screen</button>}
        </div>
        {current.controls.map((control, index) => <ControlEditor key={control.id} control={control} mine={mine} saved={!dirty}
          onChange={next => editScreen(list => list.map((c, i) => i === index ? next : c))}
          onMove={by => editScreen(list => { const to = index + by; if (to < 0 || to >= list.length) return list; const copy = [...list]; [copy[index], copy[to]] = [copy[to], copy[index]]; return copy; })}
          onRemove={() => editScreen(list => list.filter((_, i) => i !== index))}
          onTest={() => test(control)} />)}
        {current.controls.length < 24 && <div className="row wrap"><span>Add:</span>{mine.kinds.map(kind => <button key={kind} type="button" className="small quiet" onClick={() => editScreen(list => [...list, fresh(kind, taken)])}>{KIND_NAMES[kind]}</button>)}</div>}
      </div>
      <div className="row wrap">
        <button type="button" disabled={busy || !dirty} onClick={() => act("PUT", "/api/me/board/draft", { board }, "Draft saved. Test it, then publish.")}>Save draft</button>
        {dirty && <span className="muted">Unsaved changes</span>}
      </div>
      <Status state={state} />
    </Section>
    <Section title="Test mode" intro="Test plays a control's effect here and in your OBS overlay. Nothing is charged or recorded, and viewers see nothing.">
      <div className="board-preview"><EffectStage events={shown} calm={false} /></div>
    </Section>
    <Section title="Publish" intro={mine.published_at ? `Version ${mine.version} has been live since ${new Date(mine.published_at).toLocaleString()}. Publishing starts goals over.` : "Your board isn't published yet."}>
      <ul className="list">{mine.checklist.map(c => <li key={c.label}>{c.ok ? "✓" : c.required ? "✗" : "–"} {c.label}</li>)}</ul>
      <div className="row wrap">
        <button type="button" disabled={busy || dirty || required.length > 0} title={dirty ? "Save the draft first" : undefined} onClick={() => act("POST", "/api/me/board/publish", undefined, "Published.")}>Publish</button>
        {mine.published && <button type="button" className="quiet" disabled={busy} onClick={async () => { const r = await send("PUT", `${live}/disabled`, { disabled: !mine.disabled }); if (r.ok) await load(); else setState({ error: r.error }); }}>{mine.disabled ? "Resume the board" : "Pause all effects (panic)"}</button>}
      </div>
    </Section>
    <Section title="OBS overlay" intro="Add this private URL as a browser source over your whole scene (1920×1080). While it's connected, effects appear in your video instead of over the player, so every viewer sees them in sync.">
      <p>{mine.overlay.connected ? "✓ Connected" : mine.overlay.set ? "Not connected right now" : "No overlay URL yet"}</p>
      {overlayUrl && <p className="form-message">Copy it now; it won&apos;t be shown again. <code>{overlayUrl}</code> <CopyButton value={overlayUrl} label="overlay URL" /></p>}
      <div className="row wrap">
        <button type="button" className="small" disabled={busy} onClick={async () => { if (mine.overlay.set && !window.confirm("Make a new URL? The old one stops working.")) return; const r = await send<{ url: string }>("POST", "/api/me/board/overlay"); if (r.ok) { setOverlayUrl(r.data.url); await load(); } else setState({ error: r.error }); }}>{mine.overlay.set ? "Make a new URL" : "Make the overlay URL"}</button>
        {mine.overlay.set && <button type="button" className="small quiet" disabled={busy} onClick={() => { setOverlayUrl(null); void act("DELETE", "/api/me/board/overlay", undefined, "Overlay URL turned off.", dirty); }}>Turn off</button>}
      </div>
    </Section>
    <Section title="Moderators and webhook" intro="A webhook sends each press to your own HTTPS endpoint, signed with a secret (header SVER-Signature: t=timestamp,v1=HMAC-SHA256 of “timestamp.body”). Private network addresses are refused.">
      <form onSubmit={async e => {
        e.preventDefault();
        const data = await act("PUT", "/api/me/board/settings", { moderators_run: mine.moderators_run, webhook_url: webhook || null }, "Saved.", dirty);
        if (data?.webhook_secret) setSecret(data.webhook_secret);
      }}>
        <label className="checkbox"><input type="checkbox" checked={mine.moderators_run} onChange={e => void act("PUT", "/api/me/board/settings", { moderators_run: e.target.checked, webhook_url: mine.webhook_url }, "Saved.", dirty)} /> Let my channel moderators pause the board and block viewers from it</label>
        <label className="field"><span>Webhook URL (optional)</span><input type="url" value={webhook} maxLength={500} placeholder="https://example.com/sver" onChange={e => setWebhook(e.target.value)} /></label>
        <button className="small" disabled={busy}>Save webhook</button>
      </form>
      {secret && <p className="form-message">Signing secret, shown once: <code>{secret}</code> <CopyButton value={secret} label="signing secret" /></p>}
    </Section>
    <SkillSettings />
  </>;
}

const CATEGORY_NAMES: Record<string, string> = { sticker: "Stickers", fullscreen: "Full-screen moments", sound: "Sounds" };
/** Skill categories viewers can play on this channel (docs/CROWDSYNC.md "Skills"). */
function SkillSettings() {
  const [data, setData] = useState<{ categories: string[]; disabled: string[] } | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<{ categories: string[]; disabled: string[] }>("GET", "/api/me/skills");
    if (r.ok) setData(r.data);
  }, []);
  useLoad(load);
  if (!data) return null;
  async function toggle(category: string, on: boolean) {
    const disabled = on ? data!.disabled.filter(c => c !== category) : [...data!.disabled, category];
    const r = await send<{ categories: string[]; disabled: string[] }>("PUT", "/api/me/skills", { disabled });
    if (r.ok) { setData(r.data); setState({ saved: "Saved." }); } else setState({ error: r.error });
  }
  return <Section title="Skills" intro="Viewers can play premium effects bought with Purchased Valor; you earn 0.8¢ per Valor, like a tribute. Switch off any kind you don't want on your stream. Pausing the board pauses Skills too.">
    {data.categories.map(c => <label key={c} className="checkbox"><input type="checkbox" checked={!data.disabled.includes(c)} onChange={e => void toggle(c, e.target.checked)} /> {CATEGORY_NAMES[c] ?? c}</label>)}
    <Status state={state} />
  </Section>;
}
