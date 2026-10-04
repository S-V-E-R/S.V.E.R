"use client";
import { useRouter } from "next/navigation";
import { FormEvent, useState } from "react";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { ReportButton, TakeDownLink } from "./Report";
import { UserChip } from "./UserChip";

export type FanArtItem = { id: string; image: Record<string, string>; artist_name: string; artist_link: string | null; caption: string; status: string; submitted_at: string; submitter: Chip; can_delete: boolean; can_report: boolean };
export type FanArtPage = { enabled: boolean; items: FanArtItem[]; next_cursor: string | null; viewer: { signed_in: boolean; is_owner: boolean; verified: boolean } };

function Submit({ username }: { username: string }) {
  const router = useRouter();
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    setBusy(true);
    const result = await send("POST", `/api/channels/${encodeURIComponent(username)}/fan-art`, new FormData(form));
    setBusy(false);
    setMessage(result.ok ? "Submitted. It appears here once the channel owner approves it." : result.error);
    if (result.ok) { form.reset(); router.refresh(); }
  }
  return <form className="panel fan-art-form" onSubmit={submit}>
    <h3>Submit fan art</h3>
    <label className="field"><span>Image (JPG, PNG or WebP, up to 5 MB)</span><input type="file" name="file" accept="image/jpeg,image/png,image/webp" required /></label>
    <label className="field"><span>Artist name (optional)</span><input name="artist_name" maxLength={80} /></label>
    <label className="field"><span>Artist link (optional)</span><input name="artist_link" type="url" placeholder="https://" /></label>
    <label className="field"><span>Caption (optional)</span><input name="caption" maxLength={200} /></label>
    <label className="checkbox"><input type="checkbox" name="attest" value="true" required /> I made this or have permission to share it.</label>
    <button type="submit" className="small" disabled={busy}>Submit</button>
    {message && <p role="status" className="form-message">{message}</p>}
  </form>;
}

export function FanArtGallery({ username, page }: { username: string; page: FanArtPage }) {
  const router = useRouter();
  async function remove(id: string) {
    if (!window.confirm("Remove this fan art?")) return;
    const result = await send("DELETE", `/api/fan-art/${id}`);
    if (result.ok) router.refresh();
  }
  return <>
    {page.enabled && page.viewer.signed_in && !page.viewer.is_owner && (page.viewer.verified ? <Submit username={username} /> : <p className="muted">Verify your email to submit fan art.</p>)}
    {page.items.length === 0 ? <p className="muted">No fan art yet.</p> : <ul className="gallery">{page.items.map(item => <li key={item.id} className="panel">
      <a href={item.image["1600"]} target="_blank" rel="noopener">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={item.image["400"]} alt={item.caption || `Fan art by ${item.artist_name}`} loading="lazy" width={400} />
      </a>
      <p>{item.artist_link ? <a href={item.artist_link} rel="nofollow noopener noreferrer ugc" target="_blank">{item.artist_name}</a> : item.artist_name}</p>
      {item.caption && <p className="muted">{item.caption}</p>}
      <div className="meta"><UserChip user={item.submitter} size={20} />{item.status !== "APPROVED" && <span className="badge">{item.status === "PENDING" ? "Waiting for approval" : "Not approved"}</span>}{item.can_delete && <button type="button" className="link-button" onClick={() => remove(item.id)}>{page.viewer.is_owner ? "Remove" : "Withdraw"}</button>}{item.can_report ? <ReportButton target={{ target_type: "fan_art", target_id: item.id }} /> : <TakeDownLink target={{ target_type: "fan_art", target_id: item.id }} />}</div>
    </li>)}</ul>}
  </>;
}
