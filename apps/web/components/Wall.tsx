"use client";
import { HeartIcon } from "./shell/Icons";
import { useRouter } from "next/navigation";
import { FormEvent, useState } from "react";
import { send } from "../lib/client-api";
import type { Post, Reply, WallViewer } from "../lib/types";
import { ReportButton, TakeDownLink } from "./Report";
import { Ago, Linkified } from "./Text";
import { UserChip } from "./UserChip";
import { Crest } from "./Crest";

/** The fixed sticker set a signature can carry (docs/PROFILES.md "The Wall, signed"). Inline SVG only. */
export const STICKERS: Record<string, [string, React.ReactNode]> = {
  torch: ["Torch", <path key="t" d="M10 21h4l1-9H9zM12 3c3 3 3 6 0 8-3-2-3-5 0-8z" />],
  crown: ["Crown", <path key="c" d="M4 18h16l-1-10-4 3-3-6-3 6-4-3z" />],
  moon: ["Moon", <path key="m" d="M15 3a9 9 0 1 0 6 13A7 7 0 0 1 15 3z" />],
  flame: ["Flame", <path key="f" d="M12 2c4 5 6 8 6 12a6 6 0 0 1-12 0c0-3 2-5 3-7 1 2 2 3 3 3 0-3-1-5 0-8z" />],
  star: ["Star", <path key="s" d="M12 2l3 7 7 .7-5.3 4.8 1.6 7.5L12 18l-6.3 4 1.6-7.5L2 9.7 9 9z" />],
  heart: ["Heart", <path key="h" d="M12 21s-8-5-8-11a4.5 4.5 0 0 1 8-2.8A4.5 4.5 0 0 1 20 10c0 6-8 11-8 11z" />],
  gg: ["GG", <text key="g" x="12" y="16" textAnchor="middle" fontSize="11" fontWeight="800" fill="currentColor" stroke="none">GG</text>],
  wave: ["Wave", <path key="w" d="M7 13V6a1.5 1.5 0 0 1 3 0v5V4a1.5 1.5 0 0 1 3 0v7V5a1.5 1.5 0 0 1 3 0v8l1-2a1.5 1.5 0 0 1 2.6 1.5L16 19a5 5 0 0 1-4.5 3H11a5 5 0 0 1-5-5v-4a1.5 1.5 0 0 1 3 0" />],
};
export function Sticker({ name, size = 28 }: { name: string; size?: number }) {
  const s = STICKERS[name];
  if (!s) return null;
  return <svg className="sticker" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" role="img" aria-label={`${s[0]} sticker`}>{s[1]}</svg>;
}

function Composer({ path, limit, placeholder, onSaved, stickers = false }: { path: string; limit: number; placeholder: string; onSaved: (status: string, label: string | null) => void; stickers?: boolean }) {
  const [body, setBody] = useState("");
  const [sticker, setSticker] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    const result = await send<{ status: string; status_label: string | null }>("POST", path, stickers && sticker ? { body, sticker } : { body });
    setBusy(false);
    if (!result.ok) return setError(result.error);
    setBody(""); setError(""); setSticker("");
    onSaved(result.data.status, result.data.status_label);
  }
  return <form className="composer" onSubmit={submit}>
    <label className="field"><span className="sr-only">{placeholder}</span><textarea value={body} onChange={e => setBody(e.target.value)} maxLength={limit} rows={3} placeholder={placeholder} required /></label>
    {stickers && <div className="sticker-picker" role="radiogroup" aria-label="Add a sticker (optional)">{Object.keys(STICKERS).map(k => <button key={k} type="button" role="radio" aria-checked={sticker === k} className="sticker-choice" onClick={() => setSticker(sticker === k ? "" : k)}><Sticker name={k} size={22} /></button>)}</div>}
    <div className="row"><small className="muted">{[...body].length}/{limit}</small><button type="submit" className="small" disabled={busy || !body.trim()}>Post</button></div>
    {error && <p role="alert" className="form-message">{error}</p>}
  </form>;
}

function ReplyItem({ reply, onChange }: { reply: Reply; onChange: () => void }) {
  async function remove() {
    if (!window.confirm("Delete this reply?")) return;
    const result = await send("DELETE", `/api/wall/replies/${reply.id}`);
    if (result.ok) onChange();
  }
  return <li className="reply">
    <UserChip user={reply.author} size={24} />
    {reply.body === null ? <p className="muted">{reply.status === "REMOVED" ? "Removed by S.V.E.R moderators." : "This reply is unavailable."}</p> : <p className="wall-body"><Linkified text={reply.body} /></p>}
    <div className="meta"><Ago iso={reply.created_at} />{reply.status_label && <span className="badge">{reply.status_label}</span>}{reply.can_delete && <button type="button" className="link-button" onClick={remove}>Delete</button>}{reply.can_report ? <ReportButton target={{ target_type: "wall_reply", target_id: reply.id }} /> : <TakeDownLink target={{ target_type: "wall_reply", target_id: reply.id }} />}</div>
  </li>;
}

/** One wall post with likes, replies and its own report/delete controls. */
export function WallPost({ post, viewer, wallOwner }: { post: Post; viewer: WallViewer; wallOwner?: string }) {
  const router = useRouter();
  const [liked, setLiked] = useState(post.liked);
  const [likes, setLikes] = useState(post.like_count);
  const [replies, setReplies] = useState<Reply[]>(post.replies);
  const [cursor, setCursor] = useState<string | null | undefined>(post.more_replies ? undefined : null);
  const [replying, setReplying] = useState(false);
  const [notice, setNotice] = useState("");
  async function like() {
    const result = await send<{ liked: boolean; like_count: number }>(liked ? "DELETE" : "PUT", `/api/wall/posts/${post.id}/like`);
    if (result.ok) { setLiked(result.data.liked); setLikes(result.data.like_count); } else setNotice(result.error);
  }
  async function more() {
    const query = cursor ? `?cursor=${encodeURIComponent(cursor)}` : "";
    const result = await send<{ items: Reply[]; next_cursor: string | null }>("GET", `/api/wall/posts/${post.id}/replies${query}`);
    if (!result.ok) return;
    setReplies(cursor ? [...replies, ...result.data.items] : result.data.items);
    setCursor(result.data.next_cursor);
  }
  async function remove() {
    if (!window.confirm("Delete this post and its replies?")) return;
    const result = await send("DELETE", `/api/wall/posts/${post.id}`);
    if (result.ok) router.refresh();
  }
  async function pin() {
    const mine = await send<{ pins: { id: string }[] }>("GET", "/api/me/wall");
    if (!mine.ok) return setNotice(mine.error);
    const ids = mine.data.pins.map(p => p.id).filter(id => id !== post.id);
    const result = await send("PUT", "/api/me/wall/pins", { post_ids: post.pinned_position ? ids : [...ids, post.id] });
    if (result.ok) router.refresh(); else setNotice(result.error);
  }
  const hidden = post.body === null;
  // The owner's own posts on their own page: no LIVE badge on their name (docs/PROFILES.md "The Wall, signed").
  const own = !!wallOwner && post.author.username === wallOwner;
  const author = own ? { ...post.author, live: false } : post.author;
  return <article className="wall-post panel" data-faction={own ? undefined : post.author.faction ?? undefined}>
    {!own && post.author.faction && <span className="wall-stamp" aria-hidden="true"><Crest faction={post.author.faction} initial="" size={30} /></span>}
    <header className="row">{post.pinned_position && <span className="badge">Pinned</span>}{post.first_to_sign && <span className="badge first-sign">First to sign</span>}<UserChip user={author} /><Ago className="muted" iso={post.created_at} /></header>
    {post.sticker && !hidden && <span className="wall-sticker"><Sticker name={post.sticker} size={34} /></span>}
    {hidden ? <p className="muted">{post.status === "REMOVED" ? "Removed by S.V.E.R moderators." : "This post is unavailable."}</p> : <p className="wall-body"><Linkified text={post.body ?? ""} /></p>}
    {post.status_label && <span className="badge">{post.status_label}</span>}
    <div className="meta">
      <button type="button" className="link-button" aria-pressed={liked} disabled={!viewer.can_react || post.status !== "APPROVED"} onClick={like}><HeartIcon filled={liked} /> {liked ? "Liked" : "Like"} · {likes}</button>
      {viewer.can_post && post.status === "APPROVED" && <button type="button" className="link-button" onClick={() => setReplying(!replying)}>Reply</button>}
      {post.can_delete && <button type="button" className="link-button" onClick={remove}>Delete</button>}
      {post.can_report && <ReportButton target={{ target_type: "wall_post", target_id: post.id }} />}
      {!post.can_report && !post.can_delete && <TakeDownLink target={{ target_type: "wall_post", target_id: post.id }} />}
      {post.can_pin && <button type="button" className="link-button" onClick={pin}>{post.pinned_position ? "Unpin" : "Pin"}</button>}
    </div>
    {notice && <p role="alert" className="form-message">{notice}</p>}
    {replies.length > 0 && <ul className="replies">{replies.map(r => <ReplyItem key={r.id} reply={r} onChange={() => router.refresh()} />)}</ul>}
    {cursor !== null && post.reply_count > replies.length && <button type="button" className="link-button" onClick={more}>Show more replies</button>}
    {replying && <Composer path={`/api/wall/posts/${post.id}/replies`} limit={300} placeholder="Write a reply" onSaved={(_, label) => { setReplying(false); setNotice(label || ""); router.refresh(); }} />}
  </article>;
}

/** "Sign the Wall" composer, or the reason posting isn't available. */
export function WallComposer({ username, viewer }: { username: string; viewer: WallViewer }) {
  const router = useRouter();
  const [notice, setNotice] = useState("");
  if (!viewer.can_post) return viewer.reason ? <p className="muted wall-reason">{viewer.reason}</p> : null;
  return <div className="panel">
    <Composer stickers={!viewer.is_owner} path={`/api/channels/${encodeURIComponent(username)}/wall`} limit={500} placeholder="Sign the Wall" onSaved={(_, label) => { setNotice(label ? `Posted. ${label}.` : "Posted."); router.refresh(); }} />
    {notice && <p role="status" className="form-message">{notice}</p>}
  </div>;
}
