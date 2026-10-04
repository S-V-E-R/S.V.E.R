"use client";
import { KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { send } from "../lib/client-api";
import { kindLabel } from "../lib/setup-parts";

export type Part = { id: string; brand: string; model: string; name: string; kind?: string | null };
export type Pick = { part_id: string | null; name: string; kind?: string | null };
const norm = (s: string) => s.toLowerCase().replaceAll("+", " plus ").replace(/[^\p{L}\p{N}]+/gu, " ").trim();

/** Search-as-you-type box over SVER's parts list for one setup category (ARIA combobox). The last
 *  option adds what was typed as a custom entry. Picking calls onPick and clears the box. */
export function PartPicker({ category, label, disabled, onPick }: { category: string; label: string; disabled?: boolean; onPick: (pick: Pick) => void }) {
  const id = useId();
  const [q, setQ] = useState("");
  const [open, setOpen] = useState(false);
  const [parts, setParts] = useState<Part[]>([]);
  const [active, setActive] = useState(-1);
  const [error, setError] = useState("");
  const seq = useRef(0);
  const typed = q.trim().replace(/\s+/g, " ");
  const exact = parts.some(p => norm(p.name) === norm(typed));
  const options: (Part | { custom: string })[] = [...parts, ...(typed && !exact ? [{ custom: typed.slice(0, 80) }] : [])];
  useEffect(() => {
    if (!open) return;
    const n = ++seq.current;
    const t = setTimeout(async () => {
      const r = await send<{ parts: Part[] }>("GET", `/api/parts?category=${encodeURIComponent(category)}&q=${encodeURIComponent(q.slice(0, 80))}`);
      if (n !== seq.current) return;
      if (r.ok) { setParts(r.data.parts); setError(""); } else { setParts([]); setError(r.error); }
    }, 150);
    return () => clearTimeout(t);
  }, [q, open, category]);
  function choose(o: Part | { custom: string }) {
    onPick("custom" in o ? { part_id: null, name: o.custom } : { part_id: o.id, name: o.name, kind: o.kind ?? null });
    setQ(""); setActive(-1); setOpen(false);
  }
  function key(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") { e.preventDefault(); setOpen(true); setActive(Math.min(active + 1, options.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive(Math.max(active - 1, 0)); }
    else if (e.key === "Enter") {
      e.preventDefault();
      const o = options[active] ?? (exact ? parts.find(p => norm(p.name) === norm(typed)) : options[options.length - 1]);
      if (o && open) choose(o);
    } else if (e.key === "Escape") { setOpen(false); setActive(-1); }
  }
  const list = `${id}-list`;
  return <div className="part-picker">
    <label className="field"><span>Add to {label}</span>
      <input role="combobox" aria-expanded={open && options.length > 0} aria-controls={list} aria-autocomplete="list" aria-activedescendant={open && active >= 0 ? `${id}-${active}` : undefined}
        value={q} disabled={disabled} maxLength={80} placeholder={`Search ${label} or type your own`} autoComplete="off"
        onChange={e => { setQ(e.target.value); setOpen(true); setActive(-1); }} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)} onKeyDown={key} />
    </label>
    {open && options.length > 0 && <ul role="listbox" id={list} className="part-options" aria-label={`${label} parts`}>
      {options.map((o, i) => <li key={"custom" in o ? "custom" : o.id} id={`${id}-${i}`} role="option" aria-selected={i === active} className={i === active ? "active" : undefined}
        onMouseDown={e => { e.preventDefault(); choose(o); }} onMouseEnter={() => setActive(i)}>
        {"custom" in o ? <span>Add “{o.custom}” as a custom entry <small className="muted">(staff may add it to the list)</small></span> : <span><strong>{o.brand}</strong> {o.model}{kindLabel(o.kind) && <small className="muted"> · {kindLabel(o.kind)}</small>}</span>}
      </li>)}
    </ul>}
    {open && !typed && parts.length === 0 && !error && <p className="muted small-print" aria-live="polite">Start typing to search.</p>}
    {error && <p className="form-message" role="alert">{error}</p>}
  </div>;
}
