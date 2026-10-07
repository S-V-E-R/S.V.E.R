"use client";
import { useState } from "react";
import { send } from "../lib/client-api";

type Evidence = { beacon: { hidden: boolean; status: string; title: string }; playback: string | null; thumbnail: string | null };

/** Staff evidence player for a reported Beacon, with hide and return (docs/BEACONS.md "Reports"). Opening it is audited. */
export function BeaconEvidence({ id }: { id: string }) {
  const [data, setData] = useState<Evidence | null>(null);
  const [message, setMessage] = useState("");
  async function open() {
    const result = await send<Evidence>("GET", `/api/admin/beacons/${id}/review`);
    if (result.ok) setData(result.data); else setMessage(result.error);
  }
  async function hide(hidden: boolean) {
    const reason = window.prompt(hidden ? "Why hide this Beacon from every feed?" : "Why return this Beacon to the feeds?");
    if (!reason) return;
    const result = await send("POST", `/api/admin/beacons/${id}/hide`, { hidden, reason });
    setMessage(result.ok ? (hidden ? "Hidden from every feed." : "Returned to the feeds.") : result.error);
    if (result.ok) await open();
  }
  return <div>
    {!data ? <button type="button" className="small quiet" onClick={() => void open()}>Review Beacon evidence</button> : <>
      {data.playback ? <video src={data.playback} poster={data.thumbnail ?? undefined} controls playsInline style={{ maxHeight: 480, aspectRatio: "9 / 16", background: "#000" }} aria-label={`Beacon: ${data.beacon.title}`} /> : <p className="muted">No playable copy ({data.beacon.status.toLowerCase()}).</p>}
      <p>{data.beacon.hidden ? "Hidden from feeds." : "Still playing: an open report doesn't hide a Beacon."} <button type="button" className="small quiet" onClick={() => void hide(!data.beacon.hidden)}>{data.beacon.hidden ? "Return to feeds" : "Hide from feeds"}</button></p>
    </>}
    {message && <p role="status">{message}</p>}
  </div>;
}
