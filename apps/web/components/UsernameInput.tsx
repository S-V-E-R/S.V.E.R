"use client";
import { useEffect, useRef, useState } from "react";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { Autocomplete } from "./Autocomplete";

/**
 * A username field that suggests accounts as you type (GET /api/search, 200 ms after typing stops).
 * Drop-in for `<input name="username">` in uncontrolled forms: it submits under `name` and clears
 * on the form's reset.
 */
export function UsernameInput({ label, name = "username", required, disabled, exclude = [], value: controlled, onChange, hint }: {
  label: string; name?: string; required?: boolean; disabled?: boolean; exclude?: (string | null | undefined)[];
  value?: string; onChange?: (value: string) => void; hint?: string;
}) {
  const [own, setOwn] = useState("");
  const value = controlled ?? own;
  const set = (v: string) => { setOwn(v); onChange?.(v); };
  const [result, setResult] = useState<{ query: string; channels: Chip[]; error: string } | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const query = value.trim().replace(/^@/, "");
  useEffect(() => {
    if (query.length < 2 || disabled) return;
    let stopped = false;
    const timer = setTimeout(async () => {
      const response = await send<{ channels: Chip[] }>("GET", `/api/search?q=${encodeURIComponent(query)}`);
      if (!stopped) setResult({ query, channels: response.ok ? response.data.channels : [], error: response.ok ? "" : response.error });
    }, 200);
    return () => { stopped = true; clearTimeout(timer); };
  }, [query, disabled]);
  useEffect(() => {
    const form = box.current?.closest("form");
    const reset = () => { setOwn(""); onChange?.(""); };
    form?.addEventListener("reset", reset);
    return () => form?.removeEventListener("reset", reset);
  }, [onChange]);
  const current = result?.query === query ? result : null;
  const options = (current?.channels ?? []).filter(c => c.username && !exclude.some(n => n?.toLowerCase() === c.username?.toLowerCase()))
    .slice(0, 10).map(c => ({ value: c.username!, label: c.display_name === c.username ? `@${c.username}` : `${c.display_name} · @${c.username}` }));
  return <div ref={box}>
    <Autocomplete label={label} name={name} required={required} maxLength={26} disabled={disabled} value={value} onChange={set} options={options} onPick={o => set(o.value)} />
    {hint !== undefined && <p className="small-print" role="status">{query.length < 2 ? hint : !current ? "Searching…" : current.error || (!options.length ? "No matching accounts." : "Choose an account.")}</p>}
  </div>;
}
