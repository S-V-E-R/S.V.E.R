"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../Form";
import { TrailerCard } from "../TrailerCard";
import { send, useLoad } from "../../lib/client-api";
import type { Trailer } from "../../lib/types";

/** Your page → Sound: the channel trailer, one of your own public clips or Highlights or a YouTube video. */
export function TrailerEditor({ username, onSaved }: { username: string; onSaved?: () => void }) {
  const [trailer, setTrailer] = useState<Trailer | undefined>();
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<{ channel: { trailer: Trailer } }>("GET", `/api/channels/${encodeURIComponent(username)}`);
    if (r.ok) setTrailer(r.data.channel.trailer);
  }, [username]);
  useLoad(load);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setState({ saved: "Checking the video…" });
    const r = await send<{ trailer: Trailer }>("PUT", "/api/me/trailer", { url: new FormData(event.currentTarget).get("url") });
    setState(r.ok ? { saved: "Trailer saved." } : r);
    if (r.ok) { setTrailer(r.data.trailer); onSaved?.(); }
  }
  async function remove() {
    const r = await send("DELETE", "/api/me/trailer");
    setState(r.ok ? { saved: "Trailer removed." } : r);
    if (r.ok) { setTrailer(null); onSaved?.(); }
  }
  if (trailer === undefined) return <p className="loading">Loading…</p>;
  return <Section title="Channel trailer" intro="A short intro video so visitors get a feel for you before they watch. Paste a link to one of your own public clips or Highlights, or a YouTube video. It sits at the top of your Home and in the player while you're offline, and only plays when someone presses Play.">
    <form onSubmit={save}>
      <label className="field"><span>Clip, Highlight or YouTube link</span><input name="url" type="url" required defaultValue={trailer?.url || ""} placeholder="https://sver.tv/clips/… or https://www.youtube.com/watch?v=…" /></label>
      <div className="row"><button type="submit" className="small">Save trailer</button>{trailer && <button type="button" className="small quiet" onClick={remove}>Remove trailer</button>}</div>
      <Status state={state} />
    </form>
    {trailer && <TrailerCard key={trailer.media_id} trailer={trailer} />}
  </Section>;
}
