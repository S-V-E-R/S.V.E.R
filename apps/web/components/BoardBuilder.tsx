"use client";
/* Creator Studio's visual board builder (docs/CROWDSYNC.md "Builder and Studio layout"): the
   4-column grid to drag, resize and pick controls on, the side panel that edits the picked one, and
   the viewer preview. The logic lives in lib/board-builder.ts; the page keeps the undo history. */
import { useRef, useState } from "react";
import { ArrowIcon } from "./shell/Icons";
import { BOARD_COLORS, BOARD_ICONS, BoardPreview, ControlFace, type Control } from "./Board";
import { duplicate, fresh, KIND_NAMES, MAX_CONTROLS, move, widthAt } from "../lib/board-builder";

export type BuilderOptions = {
  effects: string[]; custom_effects: { value: string; name: string }[]; kinds: Control["kind"][]; audiences: Control["audience"][];
  colors: string[]; icons: string[]; typical_balance: number | null;
};
/** A change to the screen's controls. Changes with the same `key` in a row are one undo step. */
export type Change = (controls: Control[], key?: string) => void;
const AUDIENCE_NAMES: Record<Control["audience"], string> = { everyone: "Everyone signed in", followers: "Followers", subscribers: "Subscribers", moderators: "Moderators" };
const num = (value: string) => (value === "" ? 0 : Number(value));

export function BoardBuilder({ controls, taken, options, saved, onChange, onTest }: { controls: Control[]; taken: Set<string>; options: BuilderOptions; saved: boolean; onChange: Change; onTest: (c: Control) => void }) {
  const [picked, setPicked] = useState<string | null>(null);
  const [phone, setPhone] = useState(false);
  const grid = useRef<HTMLDivElement>(null);
  const dragging = useRef<number | null>(null);
  const index = controls.findIndex(c => c.id === picked);
  const c = index >= 0 ? controls[index] : null;
  const set = (next: Control, field: string) => onChange(controls.map((x, i) => (i === index ? next : x)), `${next.id}:${field}`);
  const shift = (by: number) => onChange(move(controls, index, index + by));

  function resize(e: React.PointerEvent<HTMLSpanElement>, at: number) {
    e.preventDefault(); e.stopPropagation();
    const handle = e.currentTarget, box = grid.current!.getBoundingClientRect(), left = handle.parentElement!.getBoundingClientRect().left - box.left;
    const key = `${controls[at].id}:resize:${e.timeStamp}`;
    handle.setPointerCapture(e.pointerId);
    let last = controls[at].width;
    handle.onpointermove = m => {
      const width = widthAt(left, m.clientX - box.left, box.width);
      if (width !== last) { last = width; onChange(controls.map((x, i) => (i === at ? { ...x, width } : x)), key); }
    };
    handle.onpointerup = () => { handle.onpointermove = null; handle.onpointerup = null; };
  }

  return <div className="builder-layout">
    <div>
      <div ref={grid} className="board-grid builder-grid">{controls.map((x, i) => <div key={x.id} role="button" tabIndex={0} aria-pressed={x.id === picked} aria-label={`${KIND_NAMES[x.kind]}: ${x.label}`}
        className={x.id === picked ? "builder-tile picked" : "builder-tile"} style={{ gridColumn: `span ${x.width || 1}` }} draggable
        onClick={() => setPicked(x.id)} onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setPicked(x.id); } }}
        onDragStart={e => { dragging.current = i; e.dataTransfer.setData("text/plain", x.id); e.dataTransfer.effectAllowed = "move"; }}
        onDragOver={e => e.preventDefault()}
        onDrop={e => { e.preventDefault(); if (dragging.current !== null && dragging.current !== i) onChange(move(controls, dragging.current, i)); dragging.current = null; }}>
        <div aria-hidden="true"><ControlFace c={x} /></div>
        <span className="builder-resize" title="Drag to resize" aria-hidden="true" onPointerDown={e => resize(e, i)} />
      </div>)}</div>
      {controls.length === 0 && <p className="muted">This screen is empty. Add a control below.</p>}
      {controls.length < MAX_CONTROLS && <div className="row wrap"><span>Add:</span>{options.kinds.map(kind => <button key={kind} type="button" className="small quiet" onClick={() => { const added = fresh(kind, taken); onChange([...controls, added]); setPicked(added.id); }}>{KIND_NAMES[kind]}</button>)}</div>}
      <p className="muted small">Drag a control to move it, drag its right edge to resize it, and click it to edit it.</p>
    </div>
    <aside className="builder-panel" aria-label="Edit control">{c ? <Editor c={c} options={options} set={set} saved={saved}
      onMove={shift} first={index === 0} last={index === controls.length - 1} full={controls.length >= MAX_CONTROLS}
      onDuplicate={() => { const next = duplicate(controls, index, taken); onChange(next); setPicked(next[index + 1].id); }}
      onRemove={() => { onChange(controls.filter((_, i) => i !== index)); setPicked(null); }} onTest={() => onTest(c)} />
      : <p className="muted">Click a control to edit it.</p>}</aside>
    <div className="builder-preview">
      <div className="row wrap" role="group" aria-label="Preview width">
        <strong>What viewers see</strong>
        <button type="button" className={phone ? "small quiet" : "small"} aria-pressed={!phone} onClick={() => setPhone(false)}>Desktop</button>
        <button type="button" className={phone ? "small" : "small quiet"} aria-pressed={phone} onClick={() => setPhone(true)}>Phone</button>
      </div>
      <div className={phone ? "panel board-panel board-phone" : "panel board-panel"}><BoardPreview board={{ screens: [{ name: "", controls }] }} /></div>
    </div>
  </div>;
}

function Editor({ c, options, set, saved, onMove, first, last, full, onDuplicate, onRemove, onTest }: { c: Control; options: BuilderOptions; set: (c: Control, field: string) => void; saved: boolean; onMove: (by: number) => void; first: boolean; last: boolean; full: boolean; onDuplicate: () => void; onRemove: () => void; onTest: () => void }) {
  const simple = c.kind === "label" || c.kind === "joystick" || c.kind === "rally";
  const styled = c.kind === "button" || c.kind === "text";
  return <fieldset>
    <legend>{KIND_NAMES[c.kind]} · <code>{c.id}</code></legend>
    <label className="field"><span>Label</span><input value={c.label} maxLength={40} required onChange={e => set({ ...c, label: e.target.value }, "label")} /></label>
    {!simple && <>
      <label className="field"><span>Cost (Engagement Valor, 0 is free)</span><input type="number" min={0} max={100000} value={c.cost} onChange={e => set({ ...c, cost: num(e.target.value) }, "cost")} />
        {options.typical_balance !== null && <small className={c.cost > options.typical_balance ? "form-message" : "muted"}>{c.cost > options.typical_balance ? "Above" : "Within"} the typical viewer balance here ({options.typical_balance.toLocaleString()}).</small>}</label>
      <label className="field"><span>Cooldown per viewer (seconds)</span><input type="number" min={0} max={3600} value={c.cooldown_seconds} onChange={e => set({ ...c, cooldown_seconds: num(e.target.value) }, "cooldown")} /></label>
      <label className="field"><span>Limit per stream</span><input type="number" min={1} max={1000} placeholder="No limit" value={c.per_stream_limit ?? ""} onChange={e => set({ ...c, per_stream_limit: e.target.value ? Number(e.target.value) : null }, "limit")} /></label>
      <label className="field"><span>Effect</span><select disabled={c.confirm} value={c.effect} onChange={e => set({ ...c, effect: e.target.value }, "effect")}>{options.effects.map(fx => <option key={fx} value={fx}>{fx === "none" ? "None" : fx[0].toUpperCase() + fx.slice(1)}</option>)}{options.custom_effects.length > 0 && <optgroup label="Your Skills">{options.custom_effects.map(fx => <option key={fx.value} value={fx.value}>{fx.name}</option>)}</optgroup>}</select></label>
    </>}
    {styled && <div className="row wrap">
      <label className="field narrow"><span>Colour</span><select value={c.color ?? ""} onChange={e => set({ ...c, color: e.target.value || undefined }, "color")}><option value="">Default</option>{options.colors.map(v => <option key={v} value={v}>{BOARD_COLORS[v]?.name ?? v}</option>)}</select></label>
      <label className="field narrow"><span>Icon</span><select value={c.icon ?? ""} onChange={e => set({ ...c, icon: e.target.value || undefined }, "icon")}><option value="">None</option>{options.icons.map(v => <option key={v} value={v}>{BOARD_ICONS[v]?.name ?? v}</option>)}</select></label>
    </div>}
    {styled && <label className="checkbox"><input type="checkbox" checked={!!c.confirm} onChange={e => set({ ...c, confirm: e.target.checked, effect: e.target.checked ? "none" : c.effect }, "confirm")} /> Game confirms: hold the viewer&apos;s Engagement Valor until your game or the OBS bridge confirms the press (refunded otherwise, after 60 seconds at most)</label>}
    {c.kind !== "label" && <label className="field"><span>Who can use it</span><select value={c.audience} onChange={e => set({ ...c, audience: e.target.value as Control["audience"] }, "audience")}>{options.audiences.map(a => <option key={a} value={a}>{AUDIENCE_NAMES[a]}</option>)}</select></label>}
    {c.kind === "goal" && <label className="field"><span>Goal</span><input type="number" min={1} max={1000000} value={c.target ?? 1} onChange={e => set({ ...c, target: num(e.target.value) }, "target")} /></label>}
    <label className="field narrow"><span>Width</span><select value={c.width} onChange={e => set({ ...c, width: Number(e.target.value) }, "width")}>{[1, 2, 3, 4].map(w => <option key={w} value={w}>{w} of 4</option>)}</select></label>
    <div className="row wrap">
      <button type="button" className="small quiet" disabled={first} onClick={() => onMove(-1)} aria-label={`Move ${c.label} earlier`}><ArrowIcon dir="left" /></button>
      <button type="button" className="small quiet" disabled={last} onClick={() => onMove(1)} aria-label={`Move ${c.label} later`}><ArrowIcon dir="right" /></button>
      <button type="button" className="small quiet" disabled={full} onClick={onDuplicate}>Duplicate</button>
      {c.kind !== "label" && c.kind !== "joystick" && <button type="button" className="small quiet" disabled={!saved} title={saved ? undefined : "Save the draft first"} onClick={onTest}>Test</button>}
      <button type="button" className="small quiet danger-text" onClick={onRemove}>Remove</button>
    </div>
  </fieldset>;
}
