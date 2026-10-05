"use client";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { send, useLoad } from "../lib/client-api";
import type { Faction } from "../lib/factions";
import { type Genre, utcDate } from "../lib/war";
import type { Chip } from "../lib/types";
import { UserChip } from "./UserChip";
import { ReportButton } from "./Report";

type Council = { my_vote: string | null; target: string | null; votes: { genre: string; votes: number }[]; closes_at: string };
type Election = { candidate: boolean; my_vote: string | null; candidates: { user: Chip; votes: number }[]; closes_at: string };
type Board = { items: { id: string; author: Chip; body: string; created_at: string; can_delete: boolean; can_report: boolean }[]; next_cursor: string | null; can_post: boolean; slow_seconds: number };
export function FactionCommunity({ faction, genres, votingOpen }: { faction: Faction; genres: Genre[]; votingOpen: boolean }) {
  const base = `/api/factions/${faction}`;
  const [board, setBoard] = useState<Board | null>(null);
  const [council, setCouncil] = useState<Council | null>(null);
  const [election, setElection] = useState<Election | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [body, setBody] = useState("");
  const pendingPost = useRef<{ id: string; body: string } | null>(null);
  const load = useCallback(async () => {
    const results = await Promise.all([send<Board>("GET", `${base}/board`), votingOpen ? send<Council>("GET", `${base}/council`) : null, votingOpen ? send<Election>("GET", `${base}/election`) : null]);
    const [b, c, e] = results;
    if (b.ok) setBoard(b.data); else setMessage(b.error);
    if (c?.ok) setCouncil(c.data); else if (c) setMessage(c.error);
    if (e?.ok) setElection(e.data); else if (e) setMessage(e.error);
  }, [base, votingOpen]);
  useLoad(load);
  useEffect(() => { const timer = setInterval(() => { if (!document.hidden && !busy) void load(); }, 30_000); return () => clearInterval(timer); }, [load, busy]);
  async function action(method: string, path: string, body: unknown) {
    setBusy(true);
    const result = await send(method, `${base}/${path}`, body);
    setMessage(result.ok ? "Saved." : result.error);
    if (result.ok) await load();
    setBusy(false);
    return result.ok;
  }
  async function post(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!pendingPost.current || pendingPost.current.body !== body) pendingPost.current = { id: crypto.randomUUID(), body };
    if (await action("POST", "board", pendingPost.current)) { setBody(""); pendingPost.current = null; }
  }
  async function older() {
    if (!board?.next_cursor) return;
    setBusy(true);
    const result = await send<Board>("GET", `${base}/board?cursor=${encodeURIComponent(board.next_cursor)}`);
    if (result.ok) setBoard({ ...result.data, items: [...board.items, ...result.data.items] }); else setMessage(result.error);
    setBusy(false);
  }
  const name = (id: string) => genres.find(g => g.id === id)?.name ?? id.replaceAll("_", " ");
  return <section className="faction-private"><h2>Members’ quarters</h2><p className="muted">Only your faction can see this council, board and election.</p>{message && <p className="notice" role="status">{message}</p>}
    <div className="contribution-columns"><section className="panel"><h3>War Council</h3>{council ? <><p>Current target: <strong>{council.target ? name(council.target) : "No target this week"}</strong></p><p>Vote for next week’s target. Voting closes {utcDate(council.closes_at)}.</p><form key={council.my_vote} onSubmit={e => { e.preventDefault(); void action("PUT", "council", { genre: new FormData(e.currentTarget).get("genre") }); }}><label className="field"><span>Target genre</span><select name="genre" required defaultValue={council.my_vote ?? ""}><option value="" disabled>Choose a genre</option>{genres.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select></label><button disabled={busy || !board?.can_post} className="small">{council.my_vote ? "Change vote" : "Vote"}</button></form><ul>{council.votes.map(v => <li key={v.genre}>{name(v.genre)}: {v.votes} vote{v.votes !== 1 && "s"}</li>)}</ul></> : <p>{votingOpen ? "Loading council…" : "Voting opens with the next season."}</p>}</section>
      <section className="panel"><h3>Board moderator election</h3><p>Verified members choose up to three moderators each week. Candidates need at least three votes and serve the following week.</p>{election ? <><p>Closes {utcDate(election.closes_at)}.</p><button disabled={busy || !board?.can_post} className="small quiet" onClick={() => action("PUT", "candidate", { enabled: !election.candidate })}>{election.candidate ? "Withdraw candidacy" : "Stand for election"}</button><form onSubmit={e => { e.preventDefault(); void action("PUT", "election", { username: new FormData(e.currentTarget).get("username") }); }} key={election.my_vote}><label className="field"><span>Your vote</span><select name="username" required defaultValue={election.my_vote ?? ""}><option value="" disabled>Choose a candidate</option>{election.candidates.map(c => <option key={c.user.username} value={c.user.username!}>{c.user.display_name} (@{c.user.username}) · {c.votes} votes</option>)}</select></label><button className="small" disabled={busy || !board?.can_post || !election.candidates.length}>Vote for moderator</button></form>{!election.candidates.length && <p>No candidates yet.</p>}</> : <p>{votingOpen ? "Loading election…" : "Elections open with the next season."}</p>}</section></div>
    <section className="panel"><div className="row between"><h3>Community board</h3><button className="small quiet" disabled={busy} onClick={() => void load()}>Refresh</button></div>{board ? <><p>500 characters per post. Slow mode: one post every {board.slow_seconds} seconds.</p>{board.can_post ? <form onSubmit={post}><label className="field"><span>Message to your faction</span><textarea value={body} onChange={e => setBody(e.target.value)} required maxLength={500} rows={3} /></label><button disabled={busy || !body.trim()} className="small">Post</button></form> : <p>Confirm your email to post or vote.</p>}<ul className="faction-posts">{board.items.map(p => <li key={p.id}><UserChip user={p.author} /><time dateTime={p.created_at}>{utcDate(p.created_at)}</time><p className="post-body">{p.body}</p><div className="row">{p.can_report && <ReportButton target={{ target_type: "faction_post", target_id: p.id }} />}{p.can_delete && <button disabled={busy} className="link-button" onClick={() => { const note = window.prompt("Remove this post? Moderators must give a reason; authors may leave it blank."); if (note !== null) void action("DELETE", `board/${p.id}`, { note }); }}>Remove</button>}</div></li>)}</ul>{!board.items.length && <p>No posts yet.</p>}{board.next_cursor && <button className="small quiet" disabled={busy} onClick={older}>Older posts</button>}</> : <p>Loading board…</p>}</section>
  </section>;
}
type Members = { items: { user: Chip; joined_at: string }[]; next_cursor: string | null };
export function RefreshFaction() {
  const router = useRouter();
  useEffect(() => { const timer = setInterval(() => { if (!document.hidden) router.refresh(); }, 30_000); return () => clearInterval(timer); }, [router]);
  return null;
}
export function MemberDirectory({ faction }: { faction: Faction }) {
  const [data, setData] = useState<Members | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => { const r = await send<Members>("GET", `/api/factions/${faction}/members`); if (r.ok) setData(r.data); else setMessage(r.error); }, [faction]);
  useLoad(load);
  async function more() { if (!data?.next_cursor) return; setBusy(true); const r = await send<Members>("GET", `/api/factions/${faction}/members?cursor=${encodeURIComponent(data.next_cursor)}`); if (r.ok) setData({ ...r.data, items: [...data.items, ...r.data.items] }); else setMessage(r.error); setBusy(false); }
  return <section><h2>Member directory</h2>{message && <p role="status">{message}</p>}{data ? <><ul className="member-directory">{data.items.map(m => <li className="panel" key={m.user.username}><UserChip user={m.user} /></li>)}</ul>{data.next_cursor && <button className="quiet" disabled={busy} onClick={more}>More members</button>}{!data.items.length && <p>No public members yet.</p>}</> : <p>Loading members…</p>}</section>;
}
