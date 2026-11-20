"use client";
import Link from "next/link";
import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { send, usePendingId } from "../lib/client-api";
import type { Chip } from "../lib/types";
import type { ChannelEmote } from "./Emote";
import { ChatLine, emoteLookup, reconnectDelay, redactReply, RetryNotice, useOutsideEmotes, useRetryWait, useSignatures, type Message } from "./Chat";
import { ReportButton, TakeDownLink } from "./Report";

// `emotes`/`allow_signatures`: the merged channel's catalog and setting (empty in MAGNet's own room).
type Snapshot = { type: "snapshot"; messages: Message[]; merged_with: Chip | null; co_stream?: boolean; holding: boolean; can_send: boolean; emotes?: ChannelEmote[]; allow_signatures?: boolean };
type Event = Snapshot | { type: "message"; message: Message } | { type: "delete"; id: string } | { type: "emotes"; emotes?: ChannelEmote[] };
const NO_MESSAGES: Message[] = [];

/**
 * A MAGNet channel's chat (docs/MAGNET.md "MAGNet chat"). While a stream is featured it is merged
 * with that channel's chat (messages carry the MAGNet mark there); it detaches when MAGNet moves on.
 */
export function HypeChat({ lane, account, upNext }: { lane: string; account: string | null; upNext: { name: string; seconds: number } | null }) {
  const [state, setState] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [wait, startWait] = useRetryWait();
  const pending = usePendingId();
  const list = useRef<HTMLOListElement>(null);
  // Follow new messages only while the reader is at the bottom of the list.
  const follow = useRef(true);
  const newestSeen = useRef(0);
  const [behind, setBehind] = useState(false);
  useEffect(() => {
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let poll: ReturnType<typeof setInterval> | undefined;
    const load = async () => { const r = await send<Snapshot>("GET", `/api/magnet/${encodeURIComponent(lane)}/chat`); if (!closed && r.ok) setState(r.data); };
    // Polling covers only the time the socket is down.
    const startPoll = () => { if (closed || poll) return; void load(); poll = setInterval(() => { if (!document.hidden) void load(); }, 4000); };
    let ws: WebSocket | null = null;
    // Drops since the last good snapshot (the backoff); the socket is retried for as long as the page is open.
    let attempt = 0;
    let everOpened = false;
    const open = () => {
      ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/magnet/${encodeURIComponent(lane)}/ws`);
      ws.onopen = () => { everOpened = true; clearInterval(poll); poll = undefined; };
      ws.onmessage = event => {
        const data = JSON.parse(event.data) as Event;
        if (data.type === "snapshot") { attempt = 0; setState(data); }
        else if (data.type === "message") setState(s => s && !s.messages.some(m => m.id === data.message.id) ? { ...s, messages: [...s.messages, data.message].slice(-100) } : s);
        else if (data.type === "delete") setState(s => s && { ...s, messages: s.messages.filter(m => m.id !== data.id).map(redactReply(data.id)) });
        else if (data.type === "emotes") { const emotes = data.emotes; if (emotes) setState(s => s && { ...s, emotes }); }
      };
      ws.onclose = event => {
        if (closed) return;
        // 4003: final (the merged chat became age-restricted for this viewer); one read shows the room.
        if (event.code === 4003) { void load(); return; }
        if (!everOpened || attempt >= 2) startPoll();
        retry = setTimeout(open, reconnectDelay(attempt, event.code));
        attempt += 1;
      };
    };
    open();
    return () => { closed = true; clearTimeout(retry); clearInterval(poll); ws?.close(); };
  }, [lane]);

  const messages = state?.messages ?? NO_MESSAGES;
  const merged = state?.merged_with?.username ?? null;
  const allowSignatures = !!state?.allow_signatures;
  const [signatures] = useSignatures(messages, allowSignatures);
  const outside = useOutsideEmotes(merged);
  const catalog = state?.emotes;
  const lookup = useMemo(() => emoteLookup([...(catalog ?? []), ...(allowSignatures ? signatures : []), ...outside]), [catalog, allowSignatures, signatures, outside]);

  // Scrolled up: a button says newer messages arrived below instead of pulling the reader down.
  useEffect(() => {
    const el = list.current;
    const newest = messages.at(-1)?.seq ?? 0;
    if (el && follow.current) { el.scrollTop = el.scrollHeight; newestSeen.current = newest; }
    else if (newest > newestSeen.current) setBehind(true);
  }, [messages]);
  const atBottom = () => { follow.current = true; setBehind(false); newestSeen.current = messages.at(-1)?.seq ?? 0; };
  const toNewest = () => { const el = list.current; if (el) el.scrollTop = el.scrollHeight; atBottom(); };

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = draft.trim();
    if (!body || busy || wait) return;
    setBusy(true); setError("");
    const result = await send<{ message: Message }>("POST", `/api/magnet/${encodeURIComponent(lane)}/chat`, { id: pending.id(body), body });
    pending.settle(result);
    if (result.ok) { setDraft(""); setState(s => s && !s.messages.some(m => m.id === result.data.message.id) ? { ...s, messages: [...s.messages, result.data.message].slice(-100) } : s); }
    else { setError(result.error); startWait(result.retry); }
    setBusy(false);
  }
  return <section className="chat panel" aria-label="MAGNet chat">
    <h2>MAGNet chat</h2>
    {state?.merged_with ? <p className="chat-system" role="status">Chatting with <Link href={`/${state.merged_with.username}`}>{state.merged_with.display_name}</Link>&apos;s {state.co_stream ? "co-stream chat; its streamers' rules apply" : "chat; their rules apply"}.</p>
      : <p className="chat-system muted">MAGNet&apos;s own room.</p>}
    {upNext && <p className="chat-system" role="status">Chat joins {upNext.name}<span aria-hidden="true"> in {upNext.seconds}…</span></p>}
    <ol className="chat-messages" ref={list} role="log" aria-live="polite" onScroll={e => { const el = e.currentTarget; const was = follow.current; follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80; if (follow.current && !was) atBottom(); }}>
      {messages.length === 0 && <li className="muted">No messages yet.</li>}
      {messages.map(m => <ChatLine key={m.id} m={m} account={account} lookup={lookup} host={state?.co_stream ? merged ?? undefined : undefined}>
        {" "}{account && m.author.username && m.author.username !== account ? <ReportButton target={{ target_type: "chat_message", target_id: m.id }} /> : <TakeDownLink target={{ target_type: "chat_message", target_id: m.id }} />}
      </ChatLine>)}
    </ol>
    {behind && <button type="button" className="small chat-newest" onClick={toNewest}>New messages below</button>}
    {!account ? <p className="chat-signin panel"><Link href="/login">Log in</Link> or <Link href="/signup">enlist</Link> to chat.</p>
      : state?.holding ? <p className="muted">Chat resumes when MAGNet moves on.</p>
        : <form onSubmit={submit} className="chat-form">
          <label htmlFor="hype-input" className="sr-only">Message</label>
          <textarea id="hype-input" value={draft} disabled={busy} maxLength={500} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
          <button type="submit" disabled={busy || !!wait}>Send</button>
          <RetryNotice wait={wait} />
          {error && <p role="alert" className="error">{error}</p>}
        </form>}
  </section>;
}
