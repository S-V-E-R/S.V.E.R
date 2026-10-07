"use client";
import Link from "next/link";
import { useState } from "react";
import { send } from "../lib/client-api";
import type { Sizes } from "../lib/types";
import { Avatar } from "./Avatar";

export type Previous = {
  reason: string; ended_at: string;
  members: { username: string; display_name: string; avatar: Sizes; live: boolean; following: boolean | null; own: boolean }[];
};

/**
 * "You just watched" (docs/MAGNET.md "After a switch"): after MAGNet moves on, the streamer the
 * viewer was just watching stays one tap away, with Follow and a link back to their channel.
 */
export function JustWatched({ previous, signedIn, onDismiss }: { previous: Previous; signedIn: boolean; onDismiss: () => void }) {
  // The viewer's own taps win over the polled state until the next switch replaces the card.
  const [followed, setFollowed] = useState<Record<string, boolean>>({});
  const [error, setError] = useState("");
  async function toggle(username: string, following: boolean) {
    const result = await send<{ following: boolean }>(following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    if (result.ok) { setFollowed(f => ({ ...f, [username]: result.data.following })); setError(""); } else setError(result.error);
  }
  return <section className="magnet-previous panel" aria-label="You just watched"><div className="magnet-previous-list">
    {previous.members.map(m => {
      const following = followed[m.username] ?? m.following ?? false;
      return <div key={m.username} className="magnet-previous-row">
        <Avatar sizes={m.avatar} name={m.display_name} size={40} />
        <p>You just watched <strong>{m.display_name}</strong><br /><span className="muted">{previous.reason}</span></p>
        <span className="row">
          {!m.own && (signedIn
            ? <button type="button" className={following ? "small quiet" : "small"} aria-pressed={following} onClick={() => toggle(m.username, following)}>{following ? "Following" : "Follow"}</button>
            : <Link className="button small" href="/login">Log in to follow</Link>)}
          <Link className="button small quiet" href={m.live ? `/${m.username}/live` : `/${m.username}`}>{m.live ? "Back to stream" : "Channel"}</Link>
        </span>
      </div>;
    })}
    {error && <p role="alert" className="form-message error">{error}</p>}
  </div>
    <button type="button" className="small quiet" aria-label="Dismiss" onClick={onDismiss}>×</button>
  </section>;
}
