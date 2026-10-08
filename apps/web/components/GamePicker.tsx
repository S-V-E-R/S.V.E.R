"use client";
import { useEffect, useState } from "react";
import { send } from "../lib/client-api";
import { Autocomplete } from "./Autocomplete";

type Category = { id: string; name: string; mature?: boolean };

/** Searches only S.V.E.R's saved catalog; an external outage cannot block this field. */
/** `onChange` gets whether the game has a mature age rating, so Studio can switch the label on. */
export function GamePicker({ value, disabled, onChange }: { value: string; disabled: boolean; onChange: (id: string, mature: boolean) => void }) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<Category[]>([]);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [loadedQuery, setLoadedQuery] = useState<string | null>(null);
  useEffect(() => {
    let stopped = false;
    async function load() {
      const params = new URLSearchParams({ q: query, include: value });
      const result = await send<{ categories: Category[] }>("GET", `/api/categories?${params}`);
      if (stopped) return;
      setLoadedQuery(query);
      if (result.ok) { setItems(result.data.categories); setError(""); }
      else setError(result.error);
    }
    // Initial choices load immediately; typing is debounced and late responses are ignored.
    const timer = setTimeout(() => void load(), query ? 250 : 0);
    return () => { stopped = true; clearTimeout(timer); };
  }, [query, value, retry]);
  return <>
    <Autocomplete label="Find a game or category" maxLength={80} value={query} disabled={disabled} onChange={setQuery} describedBy="game-search-help"
      options={query.trim() && loadedQuery === query && !error ? items.filter(item => item.id !== value).slice(0, 10).map(item => ({ value: item.id, label: item.name })) : []}
      onPick={option => { onChange(option.value, !!items.find(item => item.id === option.value)?.mature); setQuery(""); }} />
    <p id="game-search-help" className="small-print">Search by name or an alternative title. Older and upcoming games are included.</p>
    {error && <p role="alert">{error} <button type="button" className="small quiet" onClick={() => setRetry(retry + 1)}>Retry catalog</button></p>}
    <label className="field"><span>Category</span><select required value={value} disabled={disabled} onChange={event => onChange(event.target.value, !!items.find(item => item.id === event.target.value)?.mature)}>
      <option value="">Choose a category</option>
      {value && !items.some(item => item.id === value) && <option value={value}>Current category</option>}
      {items.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
    </select></label>
    <p className="small-print" role="status">{query && loadedQuery !== query ? "Searching…" : items.length === 50 ? "Showing up to 50 choices. Refine your search for more." : query && !items.some(item => item.id !== value) ? "No other matching categories. Try another name or contact support for a missing game." : `${items.length} choices.`}</p>
    <p className="small-print">Game metadata includes <a href="https://www.wikidata.org/">Wikidata</a>. Games are saved in S.V.E.R&apos;s own catalog.</p>
  </>;
}
