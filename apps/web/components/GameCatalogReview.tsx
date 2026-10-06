"use client";
import { useCallback, useState } from "react";
import { Section } from "./Form";
import { send, useLoad } from "../lib/client-api";

type Game = { id: string; name: string; genres: string[]; description: string };
type Queue = { items: Game[]; status: { last_success_at: string | null; last_complete_at: string | null; failures: number; pending: number } };

export function GameCatalogReview({ genres, changed }: { genres: { id: string; name: string }[]; changed: () => Promise<void> }) {
  const [data, setData] = useState<Queue | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const result = await send<Queue>("GET", "/api/admin/game-catalog");
    if (result.ok) { setData(result.data); setError(""); } else setError(result.error);
  }, []);
  useLoad(load);
  async function decide(id: string, genre: string | null, note: string, name?: string) {
    setBusy(true);
    const result = await send("POST", `/api/admin/game-catalog/${id}`, { genre, note, name });
    if (result.ok) { await load(); await changed(); } else setError(result.error);
    setBusy(false);
  }
  return <Section title="Game catalog updates" intro="Published genres classify games automatically. Missing or conflicting mappings appear here. Staff corrections and hidden categories are kept during refreshes.">
    {error && <p role="alert">{error}</p>}
    <button type="button" className="small quiet" disabled={busy} onClick={() => void load()}>Refresh catalog status</button>
    {data && <>
      <p className="small-print">{data.status.last_success_at ? `Last data received: ${new Date(data.status.last_success_at).toLocaleString()}.` : "No automatic update has completed yet."} {data.status.failures > 0 && "Updates are retrying; saved games remain available."}</p>
      <p>{data.status.pending} games need classification{data.status.pending > 50 && " · showing the first 50"}.</p>
      {data.items.map(game => <form key={game.id} className="panel" onSubmit={event => { event.preventDefault(); const form = new FormData(event.currentTarget); void decide(game.id, String(form.get("genre")), String(form.get("note")), String(form.get("name"))); }}>
        <p><strong>{game.name}</strong> · <a href={`https://www.wikidata.org/wiki/${game.id}`}>Source</a></p>
        <p className="muted">{game.genres.join(", ") || "No published genre"}{game.description && ` · ${game.description}`}</p>
        <label className="field"><span>Catalog name for {game.name}</span><input name="name" required maxLength={160} defaultValue={game.name} disabled={busy} /></label>
        <label className="field"><span>Genre for {game.name}</span><select name="genre" required defaultValue="" disabled={busy}><option value="" disabled>Choose a genre</option>{genres.map(genre => <option key={genre.id} value={genre.id}>{genre.name}</option>)}</select></label>
        <label className="field"><span>Reason for {game.name}</span><input name="note" required maxLength={500} disabled={busy} /></label>
        <div className="row"><button className="small" disabled={busy}>Add game</button><button type="button" className="small quiet" disabled={busy} onClick={event => { const note = String(new FormData(event.currentTarget.form!).get("note") ?? "").trim(); if (note) void decide(game.id, null, note); else setError("Enter a reason before dismissing a game."); }}>Dismiss game</button></div>
      </form>)}
    </>}
  </Section>;
}
