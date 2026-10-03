"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { SongPlayer } from "../../../../components/SongPlayer";
import { send, useLoad } from "../../../../lib/client-api";
import type { Song } from "../../../../lib/types";

export default function SongStudio() {
  const [data, setData] = useState<{ song: Song & { url?: string }; notice: string | null; revision: number } | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => { const r = await send<NonNullable<typeof data>>("GET", "/api/me/song"); if (r.ok) setData(r.data); }, []);
  useLoad(load);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setState({ saved: "Checking the track…" });
    const result = await send("PUT", "/api/me/song", { url: form.get("url"), volume: Number(form.get("volume")), revision: data?.revision });
    setState(result.ok ? { saved: "Song saved." } : result);
    if (result.ok) load();
  }
  async function remove() { const r = await send("DELETE", "/api/me/song"); setState(r.ok ? { saved: "Song removed." } : r); if (r.ok) load(); }
  if (!data) return <p className="loading">Loading…</p>;
  return <><h1>Profile song</h1>
    {data.notice && <p className="panel notice">Spotify links aren&apos;t supported. Add a YouTube or SoundCloud track.</p>}
    <Section title="Song" intro="Paste a YouTube video or SoundCloud track link. Visitors start it with a click; it never autoplays.">
      <form onSubmit={save}>
        <label className="field"><span>Track link</span><input name="url" type="url" required defaultValue={data.song?.url || ""} placeholder="https://www.youtube.com/watch?v=… or https://soundcloud.com/…" /></label>
        <label className="field narrow"><span>Volume</span><input name="volume" type="number" min={0} max={100} defaultValue={data.song?.volume ?? 50} /></label>
        <div className="row"><button type="submit" className="small">Save song</button>{data.song && <button type="button" className="small quiet" onClick={remove}>Remove song</button>}</div>
        <Status state={state} />
      </form>
      {data.song && <SongPlayer song={data.song} />}
    </Section></>;
}
