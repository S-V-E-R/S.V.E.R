"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import type { Sizes } from "../lib/types";
import { Avatar } from "./Avatar";
import { BoardPreview, type BoardDef } from "./Board";
import { Status, type SaveState } from "./Form";

/** A catalog listing's content (docs/CROWDSYNC.md "Catalog"). `board` is null for a game that sets its own. */
export type ListingContent = { name: string; summary: string; description: string; link: string; icon: Sizes; screenshots: { small: string; large: string; key?: string }[]; board: BoardDef | null };

/** A listing as its page shows it: screenshots, the description, the link and a preview of the board. */
export function ListingBody({ listing }: { listing: ListingContent }) {
  return <>
    {listing.screenshots.length > 0 && <ul className="catalog-shots">{listing.screenshots.map(s => <li key={s.small}>
      {/* eslint-disable-next-line @next/next/no-img-element -- media CDN image */}
      <a href={s.large} target="_blank" rel="noopener noreferrer"><img src={s.small} alt={`${listing.name} screenshot`} loading="lazy" /></a>
    </li>)}</ul>}
    <p className="catalog-description">{listing.description}</p>
    <p><a href={listing.link} rel="nofollow noopener noreferrer ugc" target="_blank">{listing.link}</a></p>
    {listing.board ? <><h2>The board</h2><p className="muted small">A preview. Nothing here can be pressed.</p><BoardPreview board={listing.board} /></>
      : <p className="muted">This game sets its own board when it&apos;s connected to a channel.</p>}
  </>;
}

/** "Add to my boards" on a listing's page: copies its board into the streamer's library. */
export function AddToBoards({ slug }: { slug: string }) {
  const [state, setState] = useState<SaveState & { signIn?: boolean }>({});
  const [busy, setBusy] = useState(false);
  async function add() {
    setBusy(true);
    const r = await send<{ name: string; changed: string[] }>("POST", `/api/crowdsync/${encodeURIComponent(slug)}/install`);
    setBusy(false);
    if (!r.ok) { setState(r.status === 401 ? { signIn: true } : r); return; }
    setState({ saved: `“${r.data.name}” is in your library.${r.data.changed.length ? ` These controls now have no effect: ${r.data.changed.join(", ")}.` : ""}` });
  }
  return <div>
    <button type="button" disabled={busy} onClick={add}>Add to my boards</button>
    {state.signIn && <p role="alert" className="form-message"><Link href="/login">Sign in</Link> to add this board to your library.</p>}
    <Status state={state} />
    {state.saved && <p><Link href="/studio/board">Open it in Studio → Board</Link>, load it into your draft, then publish.</p>}
  </div>;
}

type Mine = { listing: { slug: string; status: "PENDING" | "APPROVED" | "REJECTED" | "REMOVED"; reason: string | null; live: boolean; content: ListingContent } | null; max_screenshots: number };
type LibraryBoard = { id: string; name: string };
const STATUS = { PENDING: "Waiting for staff review", APPROVED: "In the catalog", REJECTED: "Staff asked for changes", REMOVED: "Removed from the catalog by staff" };

/** Settings → Developer: one app's catalog listing. Every new listing and change waits for staff review. */
export function ListingEditor({ app }: { app: { client_id: string; name: string; icon: Sizes; suspended: boolean } }) {
  const path = `/api/me/apps/${encodeURIComponent(app.client_id)}/listing`;
  const [mine, setMine] = useState<Mine | null>(null);
  const [library, setLibrary] = useState<LibraryBoard[]>([]);
  const [shots, setShots] = useState<{ key: string; small: string }[]>([]);
  const [from, setFrom] = useState("game");
  const [state, setState] = useState<SaveState>({});
  const [busy, setBusy] = useState(false);
  const show = useCallback((data: Mine) => {
    setMine(data);
    setShots((data.listing?.content.screenshots ?? []).flatMap(s => s.key ? [{ key: s.key, small: s.small }] : []));
    setFrom(data.listing ? (data.listing.content.board ? "keep" : "game") : "game");
  }, []);
  const load = useCallback(async () => {
    const r = await send<Mine>("GET", path);
    if (r.ok) show(r.data); else setState(r);
    const b = await send<{ library: LibraryBoard[] }>("GET", "/api/me/board");
    if (b.ok) setLibrary(b.data.library);
  }, [path, show]);
  useLoad(load);
  async function addShot(file: File) {
    const form = new FormData();
    form.append("file", file);
    setBusy(true);
    const r = await send<{ key: string; small: string }>("POST", `${path}/screenshot`, form);
    setBusy(false);
    if (r.ok) { setShots(s => [...s, r.data]); setState({}); } else setState(r);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const v = new FormData(event.currentTarget);
    let board: Record<string, unknown> = { from };
    if (from.startsWith("library:")) board = { from: "library", id: from.slice(8) };
    if (from === "file") {
      const file = v.get("file");
      try { board = { from: "file", file: JSON.parse(await (file as File).text()) }; }
      catch { setState({ error: "That isn't a S.V.E.R board file." }); return; }
    }
    setBusy(true);
    const r = await send<Mine>("PUT", path, { name: v.get("name"), summary: v.get("summary"), description: v.get("description"), link: v.get("link"), screenshots: shots.map(s => s.key), board });
    setBusy(false);
    if (r.ok) { show(r.data); setState({ saved: r.data.listing?.status === "APPROVED" ? "Saved. This matches what's already approved." : "Sent for review. It appears in the catalog once staff approve it." }); } else setState(r);
  }
  async function withdraw() {
    if (!window.confirm(`Take ${app.name} out of the catalog? Its listing is deleted.`)) return;
    setBusy(true);
    const r = await send<Mine>("DELETE", path);
    setBusy(false);
    if (r.ok) { show(r.data); setState({ saved: "Listing deleted." }); } else setState(r);
  }
  if (!mine) return <details><summary>Catalog listing</summary><Status state={state} /></details>;
  const listing = mine.listing, c = listing?.content;
  return <details>
    <summary>Catalog listing{listing && <> · {STATUS[listing.status]}</>}</summary>
    <p className="muted small">List this app at <Link href="/crowdsync">sver.tv/crowdsync</Link> so streamers can find it. Staff review every new listing and every change before it appears; the <Link href="/developers#catalog">publishing rules</Link> say what&apos;s allowed.</p>
    {listing && <p role="status">{STATUS[listing.status]}.{listing.reason && <> Reason: {listing.reason}</>}{listing.live && <> <Link href={`/crowdsync/${listing.slug}`}>See the public page</Link>{listing.status !== "APPROVED" && " (it shows the last approved version)"}.</>}</p>}
    {/* Remount when the saved listing changes, so the fields show what the server kept. */}
    <form className="stack" onSubmit={submit} key={JSON.stringify(c ?? null)}>
      <div className="row"><Avatar sizes={app.icon} name={app.name} size={40} /><span className="muted small">The listing uses this app&apos;s icon{app.icon ? "" : " (add one above)"}. A new icon is sent with your next change.</span></div>
      <label className="field"><span>Name</span><input name="name" required minLength={3} maxLength={40} defaultValue={c?.name ?? app.name} /></label>
      <label className="field"><span>One-line summary</span><input name="summary" required maxLength={120} defaultValue={c?.summary} /></label>
      <label className="field"><span>Description</span><textarea name="description" required maxLength={2000} rows={5} defaultValue={c?.description} /></label>
      <label className="field"><span>Link (store page or site, https://)</span><input name="link" type="url" required maxLength={500} placeholder="https://" defaultValue={c?.link} /></label>
      <div><span>Screenshots (up to {mine.max_screenshots})</span>
        {shots.length > 0 && <ul className="catalog-shots">{shots.map(s => <li key={s.key}>
          {/* eslint-disable-next-line @next/next/no-img-element -- media CDN image */}
          <img src={s.small} alt="Screenshot" />
          <button type="button" className="small quiet danger-text" onClick={() => setShots(list => list.filter(x => x.key !== s.key))}>Remove</button>
        </li>)}</ul>}
        {shots.length < mine.max_screenshots && <label className="small">Add a screenshot (up to 5 MB) <input type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onChange={e => { const f = e.currentTarget.files?.[0]; e.currentTarget.value = ""; if (f) void addShot(f); }} /></label>}
      </div>
      <label className="field"><span>Board</span><select value={from} onChange={e => setFrom(e.target.value)}>
        <option value="game">This game sets its own board</option>
        {c?.board && <option value="keep">Keep the board already on this listing</option>}
        <option value="draft">My draft in Studio → Board</option>
        {library.map(b => <option key={b.id} value={`library:${b.id}`}>From my library: {b.name}</option>)}
        <option value="file">A .sverboard.json file</option>
      </select></label>
      {from === "file" && <label className="field"><span>Board file</span><input name="file" type="file" accept=".json,application/json" required /></label>}
      {from !== "game" && <p className="muted small">Effects that use your own Skills become &ldquo;none&rdquo;, because other channels don&apos;t have them.</p>}
      <div className="row wrap">
        <button className="small" disabled={busy || app.suspended}>{listing ? "Send changes for review" : "Send for review"}</button>
        {listing && <button type="button" className="small quiet danger-text" disabled={busy} onClick={withdraw}>Delete listing</button>}
      </div>
      <Status state={state} />
    </form>
  </details>;
}
