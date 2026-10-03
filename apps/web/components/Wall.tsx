"use client";
import { useRouter } from "next/navigation";
import { FormEvent, useState } from "react";
import { send } from "../lib/client-api";
import type { Post, Reply, WallViewer } from "../lib/types";
import { ReportButton } from "./Report";
import { UserChip } from "./UserChip";

const when = (iso: string) => new Date(iso).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" });

function Composer({ path, limit, placeholder, onSaved }: { path: string; limit: number; placeholder: string; onSaved: (status: string, label: string | null) => void }) {
  const [body, setBody] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    const result = await send<{ status: string; status_label: string | null }>("POST", path, { body });
    setBusy(false);
    if (!result.ok) return setError(result.error);
    setBody(""); setError("");
    onSaved(result.data.status, result.data.status_label);
  }
  return <form className="composer" onSubmit={submit}>
    <label className="field"><span className="sr-only">{placeholder}</span><textarea value={body} onChange={e => setBody(e.target.value)} maxLength={limit} rows={3} placeholder={placeholder} required /></label>
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
    {reply.body === null ? <p className="muted">{reply.status === "REMOVED" ? "Removed by S.V.E.R moderators." : "This reply is unavailable."}</p> : <p className="wall-body">{reply.body}</p>}
    <div className="meta"><time dateTime={reply.created_at}>{when(reply.created_at)}</time>{reply.status_label && <span className="badge">{reply.status_label}</span>}{reply.can_delete && <button type="button" className="link-button" onClick={remove}>Delete</button>}{reply.can_report && <ReportButton target={{ target_type: "wall_reply", target_id: reply.id }} />}</div>
  </li>;
}

/** One wall post with likes, replies and its own report/delete controls. */
export function WallPost({ post, viewer }: { post: Post; viewer: WallViewer }) {
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
  return <article className="wall-post panel">
    <header className="row">{post.pinned_position && <span className="badge">Pinned</span>}<UserChip user={post.author} /><time className="muted" dateTime={post.created_at}>{when(post.created_at)}</time></header>
    {hidden ? <p className="muted">{post.status === "REMOVED" ? "Removed by S.V.E.R moderators." : "This post is unavailable."}</p> : <p className="wall-body">{post.body}</p>}
    {post.status_label && <span className="badge">{post.status_label}</span>}
    <div className="meta">
      <button type="button" className="link-button" aria-pressed={liked} disabled={!viewer.can_react || post.status !== "APPROVED"} onClick={like}>{liked ? "♥ Liked" : "♡ Like"} · {likes}</button>
      {viewer.can_post && post.status === "APPROVED" && <button type="button" className="link-button" onClick={() => setReplying(!replying)}>Reply</button>}
      {post.can_delete && <button type="button" className="link-button" onClick={remove}>Delete</button>}
      {post.can_report && <ReportButton target={{ target_type: "wall_post", target_id: post.id }} />}
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
    <Composer path={`/api/channels/${encodeURIComponent(username)}/wall`} limit={500} placeholder="Sign the Wall" onSaved={(_, label) => { setNotice(label ? `Posted. ${label}.` : "Posted."); router.refresh(); }} />
    {notice && <p role="status" className="form-message">{notice}</p>}
  </div>;
}
