"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { ReportForm } from "./Report";
import { UserChip } from "./UserChip";

type Comment = { id: string; body: string; created_at: string; author: Chip; mine: boolean };
type View = { comments: Comment[]; count: number; can_comment: boolean; can_moderate: boolean };

/** A Beacon's comments (docs/BEACONS.md "Comments"): newest first, the owner's chat rules, no links. */
export function BeaconComments({ id, signedIn }: { id: string; signedIn: boolean }) {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const [reporting, setReporting] = useState<string | null>(null);
  const path = `/api/beacons/${encodeURIComponent(id)}/comments`;
  const load = useCallback(async () => {
    const r = await send<View>("GET", path);
    if (r.ok) setView(r.data); else setMessage(r.error);
  }, [path]);
  useLoad(load);
  async function post(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const r = await send<View>("POST", path, { body: new FormData(form).get("body") });
    if (r.ok) { setView(r.data); setMessage(""); form.reset(); } else setMessage(r.error);
  }
  async function remove(comment: Comment) {
    const r = await send<View>("DELETE", `${path}/${encodeURIComponent(comment.id)}`);
    if (r.ok) setView(r.data); else setMessage(r.error);
  }
  if (!view) return <p className="loading">{message || "Loading comments…"}</p>;
  return <div className="beacon-comments">
    <h3>{view.count === 1 ? "1 comment" : `${view.count.toLocaleString()} comments`}</h3>
    {view.can_comment ? <form onSubmit={post} className="row">
      <label className="sr-only" htmlFor={`comment-${id}`}>Add a comment</label>
      <input id={`comment-${id}`} name="body" maxLength={300} required placeholder="Add a comment" autoComplete="off" />
      <button type="submit" className="small">Post</button>
    </form> : <p className="muted small">{signedIn ? "Verify your email address to comment." : <><Link href={`/login?next=${encodeURIComponent(`/beacons/${id}`)}`}>Sign in</Link> to comment.</>}</p>}
    {message && <p role="status" className="form-message">{message}</p>}
    <ul className="list">{view.comments.map(c => <li key={c.id}>
      <p><UserChip user={c.author} /> <span className="muted small">{new Date(c.created_at).toLocaleString()}</span></p>
      <p>{c.body}</p>
      <div className="row">
        {(c.mine || view.can_moderate) && <button type="button" className="link-button small" onClick={() => void remove(c)}>Delete</button>}
        {signedIn && !c.mine && <button type="button" className="link-button small" onClick={() => setReporting(reporting === c.id ? null : c.id)}>Report</button>}
      </div>
      {reporting === c.id && <ReportForm target={{ target_type: "beacon_comment", target_id: c.id }} onDone={() => setReporting(null)} />}
    </li>)}</ul>
  </div>;
}
