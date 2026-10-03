"use client";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";

type Message = { id: string; seq: number; author: Chip; body: string; created_at: string; role: "owner" | null };
type Event = { type: "snapshot"; messages: Message[] } | { type: "message"; message: Message } | { type: "ack"; id: string; message: Message } | { type: "error"; id?: string; message: string };

const time = (iso: string) => new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

/**
 * Channel chat. Live over the same-origin WebSocket; if the socket can't connect it polls history
 * and sends over HTTPS, which run the same server checks. Messages render as plain text.
 */
export function Chat({ username, account }: { username: string; account: string | null }) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"connecting" | "live" | "polling">("connecting");
  const socket = useRef<WebSocket | null>(null);
  const list = useRef<HTMLOListElement>(null);
  const path = `/api/channels/${encodeURIComponent(username)}/chat`;

  // Deduplicates by ID and keeps the latest 100 in server order.
  const merge = useCallback((incoming: Message[], replace = false) => {
    setMessages(current => {
      const byId = new Map((replace ? [] : current).map(m => [m.id, m]));
      for (const m of incoming) byId.set(m.id, m);
      return [...byId.values()].sort((a, b) => a.seq - b.seq).slice(-100);
    });
  }, []);

  useEffect(() => {
    let closed = false;
    let poll: ReturnType<typeof setInterval> | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const fallback = () => {
      if (closed || poll) return;
      setMode("polling");
      const load = async () => { const r = await send<{ messages: Message[] }>("GET", path); if (r.ok) merge(r.data.messages, true); };
      void load();
      poll = setInterval(() => { if (!document.hidden) void load(); }, 4000);
    };
    const open = (attempt: number) => {
      const ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/chat/ws?channel=${encodeURIComponent(username)}`);
      socket.current = ws;
      let opened = false;
      ws.onopen = () => { opened = true; setMode("live"); };
      ws.onmessage = event => {
        const data = JSON.parse(event.data) as Event;
        if (data.type === "snapshot") merge(data.messages, true);
        else if (data.type === "message" || data.type === "ack") merge([data.message]);
        else setError(data.message);
      };
      ws.onclose = () => {
        socket.current = null;
        if (closed) return;
        // Never connected: fall back to polling. Dropped (including a server resync): reconnect and reload.
        if (!opened || attempt >= 5) fallback();
        else { setMode("connecting"); retry = setTimeout(() => open(attempt + 1), 1000 * (attempt + 1)); }
      };
    };
    open(0);
    return () => { closed = true; clearInterval(poll); clearTimeout(retry); socket.current?.close(); };
  }, [path, username, merge]);

  useEffect(() => { list.current?.lastElementChild?.scrollIntoView({ block: "nearest" }); }, [messages]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = draft.trim();
    if (!body) return;
    const command = { id: crypto.randomUUID(), body };
    setError("");
    if (socket.current?.readyState === WebSocket.OPEN) {
      socket.current.send(JSON.stringify(command));
      setDraft("");
      return;
    }
    const result = await send<{ message: Message }>("POST", path, command);
    if (result.ok) { merge([result.data.message]); setDraft(""); } else setError(result.error);
  }

  return <section className="chat panel" aria-label="Chat">
    <h2>Chat {mode !== "live" && <span className="muted small">{mode === "polling" ? "(updates every few seconds)" : "(connecting…)"}</span>}</h2>
    <ol className="chat-messages" ref={list} aria-live="polite">
      {messages.length === 0 && <li className="muted">No messages yet.</li>}
      {messages.map(m => <li key={m.id}>
        <span className="muted">{time(m.created_at)}</span>{" "}
        {m.author.username ? <Link href={`/${m.author.username}`}><strong>{m.author.display_name}</strong></Link> : <strong>{m.author.display_name}</strong>}
        {m.role === "owner" && <span className="badge">Streamer</span>}: <span className="chat-body">{m.body}</span>
      </li>)}
    </ol>
    {account ? <form onSubmit={submit} className="chat-form">
      <label htmlFor="chat-input" className="sr-only">Message</label>
      <textarea id="chat-input" value={draft} maxLength={500} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
      <button type="submit">Send</button>
      {error && <p role="alert" className="error">{error}</p>}
    </form> : <p className="muted"><Link href="/login">Sign in</Link> to chat.</p>}
  </section>;
}
