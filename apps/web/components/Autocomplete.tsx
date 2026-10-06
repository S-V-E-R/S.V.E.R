"use client";
import { useEffect, useId, useState, type KeyboardEvent } from "react";

export type Suggestion = { value: string; label: string };

/** The existing setup-parts listbox style, shared by Studio's game and username searches. */
export function Autocomplete({ label, value, options, onChange, onPick, disabled, name, required, maxLength, describedBy }: {
  label: string; value: string; options: Suggestion[]; onChange: (value: string) => void; onPick: (option: Suggestion) => void;
  disabled?: boolean; name?: string; required?: boolean; maxLength: number; describedBy?: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const index = options.findIndex(option => option.value === active);
  const expanded = open && !disabled && options.length > 0;
  useEffect(() => {
    if (expanded && index >= 0) document.getElementById(`${id}-${index}`)?.scrollIntoView({ block: "nearest" });
  }, [expanded, id, index]);
  function choose(option: Suggestion) { onPick(option); setOpen(false); setActive(null); }
  function key(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault(); setOpen(true);
      const next = event.key === "ArrowDown" ? Math.min(index + 1, options.length - 1) : index < 0 ? options.length - 1 : Math.max(index - 1, 0);
      setActive(options[next]?.value ?? null);
    } else if (event.key === "Enter" && open) {
      event.preventDefault(); if (expanded) choose(options[index < 0 ? 0 : index]);
    } else if (event.key === "Escape") { event.preventDefault(); setOpen(false); setActive(null); }
  }
  return <div className="part-picker autocomplete">
    <label className="field" htmlFor={id}><span>{label}</span>
      <input id={id} name={name} required={required} maxLength={maxLength} disabled={disabled} value={value} autoComplete="off" spellCheck={false}
        role="combobox" aria-autocomplete="list" aria-expanded={expanded} aria-controls={expanded ? `${id}-list` : undefined}
        aria-activedescendant={expanded && index >= 0 ? `${id}-${index}` : undefined} aria-describedby={describedBy}
        onChange={event => { onChange(event.target.value); setOpen(true); setActive(null); }} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)} onKeyDown={key} />
    </label>
    {expanded && <ul id={`${id}-list`} role="listbox" aria-label={`${label} suggestions`} className="part-options">
      {options.map((option, i) => <li id={`${id}-${i}`} key={option.value} role="option" aria-selected={i === index}
        onMouseDown={event => event.preventDefault()} onClick={() => choose(option)} onMouseEnter={() => setActive(option.value)}>{option.label}</li>)}
    </ul>}
  </div>;
}
