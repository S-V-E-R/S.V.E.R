"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../../components/Form";
import { SongPlayer } from "../../../../components/SongPlayer";
import { send, useLoad } from "../../../../lib/client-api";
import type { Song } from "../../../../lib/types";

export default function SongStudio() {
  const [data, setData] = useState<{ song: Song & { url?: string }; notice: string | null; revision: number } | null>(null);
  const [state, setState] = useState<SaveState>({});
  const [details, setDetails] = useState<{ title: string; artist: string } | null>(null);
  const load = useCallback(async () => { const r = await send<NonNullable<typeof data>>("GET", "/api/me/song"); if (r.ok) setData(r.data); }, []);
  useLoad(load);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setState({ saved: "Checking the track…" });
    const result = await send("PUT", "/api/me/song", { url: form.get("url"), title: form.get("title"), artist: form.get("artist"), volume: Number(form.get("volume")), revision: data?.revision });
    setState(result.ok ? { saved: "Song saved." } : result);
    if (result.ok) { setDetails(null); load(); }
  }
  // Fills Title and Artist from the provider's oEmbed details; the owner can still edit both.
  async function fetchDetails(event: React.MouseEvent<HTMLButtonElement>) {
    const url = (event.currentTarget.form?.elements.namedItem("url") as HTMLInputElement | null)?.value || "";
    setState({ saved: "Checking the track…" });
    const result = await send<{ title: string; artist: string }>("POST", "/api/me/song/preview", { url });
    if (result.ok) { setDetails({ title: result.data.title, artist: result.data.artist }); setState({ saved: "Details filled in. Edit them if you like, then save." }); } else setState(result);
  }
  async function remove() { const r = await send("DELETE", "/api/me/song"); setState(r.ok ? { saved: "Song removed." } : r); if (r.ok) load(); }
  if (!data) return <p className="loading">Loading…</p>;
  return <><h1>Profile song</h1>
    {data.notice && <p className="panel notice">Spotify links aren&apos;t supported. Add a YouTube or SoundCloud track.</p>}
    <Section title="Song" intro="Paste a YouTube video or SoundCloud track link. Visitors start it with a click, at your default volume; it never autoplays.">
      <form onSubmit={save}>
        <label className="field"><span>Track link</span><input name="url" type="url" required defaultValue={data.song?.url || ""} onChange={event => { if (event.target.value.trim() !== (data.song?.url || "") && (details?.title || details?.artist || details === null)) setDetails({ title: "", artist: "" }); }} placeholder="https://www.youtube.com/watch?v=… or https://soundcloud.com/…" /></label>
        <div className="row"><button type="button" className="small quiet" onClick={fetchDetails}>Fetch details</button></div>
        <label className="field"><span>Title</span><input key={`t-${details?.title ?? data.song?.title ?? ""}`} name="title" maxLength={100} defaultValue={details?.title ?? data.song?.title ?? ""} placeholder="Filled in from the track" /></label>
        <label className="field"><span>Artist</span><input key={`a-${details?.artist ?? data.song?.artist ?? ""}`} name="artist" maxLength={100} defaultValue={details?.artist ?? data.song?.artist ?? ""} placeholder="Filled in from the track" /></label>
        <label className="field narrow"><span>Default volume (0-100)</span><input name="volume" type="number" min={0} max={100} defaultValue={data.song?.volume ?? 70} /></label>
        <div className="row"><button type="submit" className="small">Save song</button>{data.song && <button type="button" className="small quiet" onClick={remove}>Remove song</button>}</div>
        <Status state={state} />
      </form>
      {data.song && <SongPlayer song={data.song} />}
    </Section></>;
}
