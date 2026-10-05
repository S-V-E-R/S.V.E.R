"use client";
import Link from "next/link";
import { FormEvent, useEffect, useRef, useState } from "react";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { Crest } from "./FactionIdentity";
import { GuildChatBadge } from "./Guilds";

type Message = { id: string; seq: number; author: Chip; body: string; created_at: string; role: string | null; origin?: string | null };
type Snapshot = { type: "snapshot"; messages: Message[]; merged_with: Chip | null; holding: boolean; can_send: boolean };
type Event = Snapshot | { type: "message"; message: Message } | { type: "delete"; id: string };

/**
 * A Hype channel's chat (docs/MAGNET.md "Hype chat"). While a stream is featured it is merged
 * with that channel's chat (messages carry the MAGNet mark there); it detaches when MAGNet moves on.
 */
export function HypeChat({ lane, account, upNext }: { lane: string; account: string | null; upNext: { name: string; seconds: number } | null }) {
  const [state, setState] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const list = useRef<HTMLOListElement>(null);
  useEffect(() => {
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let poll: ReturnType<typeof setInterval> | undefined;
    const load = async () => { const r = await send<Snapshot>("GET", `/api/magnet/${encodeURIComponent(lane)}/chat`); if (!closed && r.ok) setState(r.data); };
    let ws: WebSocket | null = null;
    const open = (attempt: number) => {
      ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/magnet/${encodeURIComponent(lane)}/ws`);
      let opened = false;
      ws.onopen = () => { opened = true; };
      ws.onmessage = event => {
        const data = JSON.parse(event.data) as Event;
        if (data.type === "snapshot") setState(data);
        else if (data.type === "message") setState(s => s && !s.messages.some(m => m.id === data.message.id) ? { ...s, messages: [...s.messages, data.message].slice(-100) } : s);
        else if (data.type === "delete") setState(s => s && { ...s, messages: s.messages.filter(m => m.id !== data.id) });
      };
      ws.onclose = () => {
        if (closed) return;
        if (!opened || attempt >= 5) { void load(); poll = setInterval(() => { if (!document.hidden) void load(); }, 4000); }
        else retry = setTimeout(() => open(attempt + 1), 1000 * (attempt + 1));
      };
    };
    open(0);
    return () => { closed = true; clearTimeout(retry); clearInterval(poll); ws?.close(); };
  }, [lane]);
  useEffect(() => { list.current?.lastElementChild?.scrollIntoView({ block: "nearest" }); }, [state?.messages]);
  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = draft.trim();
    if (!body || busy) return;
    setBusy(true); setError("");
    const result = await send<{ message: Message }>("POST", `/api/magnet/${encodeURIComponent(lane)}/chat`, { id: crypto.randomUUID(), body });
    if (result.ok) { setDraft(""); setState(s => s && !s.messages.some(m => m.id === result.data.message.id) ? { ...s, messages: [...s.messages, result.data.message].slice(-100) } : s); }
    else setError(result.error);
    setBusy(false);
  }
  return <section className="chat panel" aria-label="Hype chat">
    <h2>Hype chat</h2>
    {state?.merged_with ? <p className="chat-system" role="status">Chatting with <Link href={`/${state.merged_with.username}`}>{state.merged_with.display_name}</Link>&apos;s chat; their rules apply.</p>
      : <p className="chat-system muted">MAGNet&apos;s own room.</p>}
    {upNext && <p className="chat-system" role="status">Chat joins {upNext.name} in {upNext.seconds}…</p>}
    <ol className="chat-messages" ref={list} aria-live="polite">
      {(state?.messages.length ?? 0) === 0 && <li className="muted">No messages yet.</li>}
      {state?.messages.map(m => <li key={m.id}>
        {m.author.faction && <Crest faction={m.author.faction} size={14} />}{" "}
        {m.author.guild && <GuildChatBadge guild={m.author.guild} />}
        {m.author.username ? <Link href={`/${m.author.username}`}><strong>{m.author.display_name}</strong></Link> : <strong>{m.author.display_name}</strong>}
        {m.origin && <span className="badge magnet-badge">MAGNet</span>}: <span className="chat-body">{m.body}</span>
      </li>)}
    </ol>
    {!account ? <p className="muted"><Link href="/login">Sign in</Link> to chat.</p>
      : state?.holding ? <p className="muted">Chat resumes when MAGNet moves on.</p>
        : <form onSubmit={submit} className="chat-form">
          <label htmlFor="hype-input" className="sr-only">Message</label>
          <textarea id="hype-input" value={draft} disabled={busy} maxLength={500} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
          <button type="submit" disabled={busy}>Send</button>
          {error && <p role="alert" className="error">{error}</p>}
        </form>}
  </section>;
}
