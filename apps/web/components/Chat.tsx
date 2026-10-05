"use client";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { ReportButton, TakeDownLink } from "./Report";
import { Crest } from "./FactionIdentity";
import { GuildChatBadge } from "./Guilds";
import type { Chip } from "../lib/types";
import { EmoteImage, type ChannelEmote } from "./Emote";
import "../styles/teams.css";

type Reply = { id: string; username: string | null; body: string | null };
type Message = { id: string; seq: number; author: Chip; body: string; created_at: string; role: "owner" | "moderator" | "staff" | null; mentions: string[]; reply: Reply | null; origin?: string | null; tribute?: number | null; sub?: { tier: number; months: number } | null };
/** Subscriber badge milestones: 1, 3, 6, 9 and 12 months, then each further year (docs/SUPPORT.md). */
export function subBadge(months: number) {
  if (months >= 24) return `${Math.floor(months / 12)} years`;
  if (months >= 12) return "1 year";
  const step = [9, 6, 3].find(m => months >= m) ?? 1;
  return `${step} ${step === 1 ? "month" : "months"}`;
}
type Snapshot = { messages: Message[]; pinned: Message | null; emotes: ChannelEmote[]; followers_only_until?: string | null; subs_only?: boolean };
type Event = ({ type: "snapshot" } & Snapshot) | { type: "emotes"; emotes: ChannelEmote[] } | { type: "pin"; pinned: Message | null } | { type: "message"; message: Message } | { type: "ack"; id: string; message: Message } | { type: "error"; id?: string; message: string } | { type: "delete"; id: string } | { type: "raid"; raid: unknown } | { type: "raid_cancelled"; id: string } | { type: "system"; text: string } | { type: "protect"; until: string | null } | { type: "subs_only"; on: boolean };

const time = (iso: string) => new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

function MessageBody({ message, account, emotes }: { message: Message; account: string | null; emotes: ChannelEmote[] }) {
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  for (const match of message.body.matchAll(/(?<!\S)[A-Za-z0-9]{3,20}(?!\S)|(?<![\p{L}\p{N}_@])@([A-Za-z0-9_]{3,25})(?![\p{L}\p{N}_])/gu)) {
    const emote = !match[1] && emotes.find(e => e.code === match[0]);
    const name = match[1] && message.mentions.find(name => name.toLowerCase() === match[1].toLowerCase());
    if (!name && !emote) continue;
    parts.push(message.body.slice(cursor, match.index));
    if (emote) parts.push(<EmoteImage key={match.index} emote={emote} />);
    else if (name) {
      const link = <Link href={`/${name}`}>{match[0]}</Link>;
      parts.push(name.toLowerCase() === account?.toLowerCase() ? <mark key={match.index} className="chat-mention">{link}</mark> : <span key={match.index}>{link}</span>);
    }
    cursor = match.index + match[0].length;
  }
  parts.push(message.body.slice(cursor));
  return <>{message.reply && <blockquote className="chat-quote">{message.reply.body === null ? "Message deleted" : <>{message.reply.username && <strong>@{message.reply.username}: </strong>}{message.reply.body}</>}</blockquote>}<span className="chat-body">{parts}</span></>;
}

/**
 * Channel chat. Live over the same-origin WebSocket; if the socket can't connect it polls history
 * and sends over HTTPS, which run the same server checks. Messages render as plain text.
 */
export function Chat({ username, account, squad }: { username: string; account: string | null; squad?: string }) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"connecting" | "live" | "polling">("connecting");
  const [role, setRole] = useState<string | null>(null);
  const [restrictions, setRestrictions] = useState<{ user: Chip; kind: string; until: string | null }[]>([]);
  const [pinned, setPinned] = useState<Message | null>(null);
  const [emotes, setEmotes] = useState<ChannelEmote[]>([]);
  const [reply, setReply] = useState<Reply | null>(null);
  const [busy, setBusy] = useState(false);
  // Valor to pay with the next message (docs/SUPPORT.md "Purchased Valor"); null when off.
  const [tribute, setTribute] = useState<number | null>(null);
  const [notice, setNotice] = useState("");
  // Spike protection: followers-only chat with an end time; moderators get a one-click prompt.
  const [followersOnly, setFollowersOnly] = useState<string | null>(null);
  const [subsOnly, setSubsOnly] = useState(false);
  const [spike, setSpike] = useState(false);
  const canPin = !squad && (role === "owner" || role === "moderator");
  const socket = useRef<WebSocket | null>(null);
  const list = useRef<HTMLOListElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const path = squad ? `/api/squads/${encodeURIComponent(squad)}/chat` : `/api/channels/${encodeURIComponent(username)}/chat`;

  // Deduplicates by ID and keeps the latest 100 in server order.
  const merge = useCallback((incoming: Message[], replace = false) => {
    setMessages(current => {
      const byId = new Map((replace ? [] : current).map(m => [m.id, m]));
      for (const m of incoming) byId.set(m.id, m);
      return [...byId.values()].sort((a, b) => a.seq - b.seq).slice(-100);
    });
  }, []);

  const remove = useCallback((id: string) => {
    const redact = (m: Message): Message => m.reply?.id === id ? { ...m, reply: { id, username: null, body: null } } : m;
    setMessages(current => current.filter(m => m.id !== id).map(redact));
    setPinned(current => current?.id === id ? null : current && redact(current));
    setReply(current => current?.id === id ? null : current);
  }, []);

  // Owner, moderators and staff get per-message actions; the server enforces every permission.
  const loadRole = useCallback(async () => {
    if (!account) return;
    const r = await send<{ role: string; spike?: boolean; followers_only_until?: string | null; restrictions?: { user: Chip; kind: string; until: string | null }[] }>("GET", `${path}/moderation`);
    if (r.ok) { setRole(r.data.role); setSpike(!!r.data.spike); setFollowersOnly(r.data.followers_only_until ?? null); setRestrictions(r.data.restrictions ?? []); }
    else setRole(null);
  }, [account, path]);
  useLoad(loadRole);
  useEffect(() => {
    if (!role) return;
    const timer = setInterval(() => { if (!document.hidden) void loadRole(); }, 15000);
    return () => clearInterval(timer);
  }, [role, loadRole]);
  // Followers-only chat always ends by itself; clear the banner at its end time.
  useEffect(() => {
    if (!followersOnly) return;
    const timer = setTimeout(() => setFollowersOnly(null), Math.max(0, Date.parse(followersOnly) - Date.now()));
    return () => clearTimeout(timer);
  }, [followersOnly]);
  async function limitToSubs(on: boolean) {
    const result = await send<{ subs_only: boolean }>("PUT", `${path}/subs-only`, { on });
    if (result.ok) setSubsOnly(result.data.subs_only); else setError(result.error);
  }
  async function protect(on: boolean) {
    const result = await send<{ followers_only_until: string | null }>("PUT", `${path}/protect`, { on });
    if (result.ok) { setFollowersOnly(result.data.followers_only_until); setSpike(false); } else setError(result.error);
  }

  useEffect(() => {
    let closed = false;
    let poll: ReturnType<typeof setInterval> | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const fallback = () => {
      if (closed || poll) return;
      setMode("polling");
      const load = async () => { const r = await send<Snapshot>("GET", path); if (!closed && r.ok) { merge(r.data.messages, true); setPinned(r.data.pinned); setEmotes(r.data.emotes ?? []); setError(""); } else if (!closed && !r.ok) { setError(r.error); if (squad) merge([], true); } };
      void load();
      poll = setInterval(() => { if (!document.hidden) void load(); }, 4000);
    };
    const open = (attempt: number) => {
      const ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/chat/ws?${squad ? `squad=${encodeURIComponent(squad)}` : `channel=${encodeURIComponent(username)}`}`);
      socket.current = ws;
      let opened = false;
      ws.onopen = () => { opened = true; setMode("live"); };
      ws.onmessage = event => {
        if (closed) return;
        const data = JSON.parse(event.data) as Event;
        if (data.type === "snapshot") { merge(data.messages, true); setPinned(data.pinned); setEmotes(data.emotes ?? []); setFollowersOnly(data.followers_only_until ?? null); setSubsOnly(!!data.subs_only); }
        else if (data.type === "protect") setFollowersOnly(data.until);
        else if (data.type === "subs_only") setSubsOnly(data.on);
        else if (data.type === "emotes") setEmotes(data.emotes);
        else if (data.type === "pin") setPinned(data.pinned);
        else if (data.type === "message" || data.type === "ack") merge([data.message]);
        else if (data.type === "delete") remove(data.id);
        // The player on this page runs the raid countdown (LivePlayer listens for this).
        else if (data.type === "raid" || data.type === "raid_cancelled") window.dispatchEvent(new CustomEvent("sver:raid", { detail: { channel: username.toLowerCase(), raid: data.type === "raid" ? data.raid : null } }));
        else if (data.type === "system") setNotice(data.text);
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
  }, [path, username, squad, merge, remove]);

  useEffect(() => { list.current?.lastElementChild?.scrollIntoView({ block: "nearest" }); }, [messages]);

  async function moderate(action: "delete" | "timeout" | "ban", m: Message) {
    const who = m.author.username;
    if (!who) return;
    let seconds: number | undefined;
    if (action === "timeout") {
      const minutes = Number(window.prompt(`Time out @${who} for how many minutes? (1–20160)`, "10"));
      if (!Number.isInteger(minutes) || minutes < 1 || minutes > 20160) return;
      seconds = minutes * 60;
    }
    if (action === "ban" && !window.confirm(`Ban @${who} from this chat? They also can't watch here while signed in.`)) return;
    const reason = window.prompt("Reason (required)")?.trim();
    if (!reason) return;
    const result = action === "delete"
      ? await send("DELETE", `${path}/messages/${m.id}`, { reason })
      : await send("POST", `${path}/restrictions`, { username: who, kind: action, seconds, reason });
    if (!result.ok) setError(result.error);
    else if (action === "delete") remove(m.id);
    else if (squad) await loadRole();
  }

  async function changePin(messageId: string | null, reason?: string) {
    reason ??= window.prompt("Reason for changing the pinned message (required)")?.trim();
    if (!reason) return;
    const result = await send<{ pinned: Message | null }>("PUT", `${path}/pin`, { message_id: messageId, reason });
    if (result.ok) setPinned(result.data.pinned); else setError(result.error);
  }

  async function submit(event: FormEvent, pinDraft = false) {
    event.preventDefault();
    const body = draft.trim();
    if (!body || busy) return;
    const reason = pinDraft ? window.prompt("Reason for pinning this message (required)")?.trim() : undefined;
    if (pinDraft && !reason) return;
    setError("");
    // Owner commands: /raid username starts a raid, /unraid cancels it during the countdown.
    if (/^\/flag$/i.test(body) && !pinDraft) {
      const result = await send("POST", "/api/me/magnet/flag");
      if (result.ok) setDraft(""); else setError(result.error);
      return;
    }
    const raid = /^\/(raid|unraid)(?:\s+@?([A-Za-z0-9_]{3,25}))?$/i.exec(body);
    if (raid && !pinDraft) {
      if (raid[1].toLowerCase() === "raid" && !raid[2]) { setError("Use /raid username."); return; }
      const result = raid[1].toLowerCase() === "raid" ? await send("POST", "/api/me/raids", { username: raid[2] }) : await send("DELETE", "/api/me/raids");
      if (result.ok) setDraft(""); else setError(result.error);
      return;
    }
    const command = { id: crypto.randomUUID(), body, reply_to: reply?.id, ...(tribute ? { tribute } : {}) };
    // Tributes go over HTTP so a refusal (balance, channel can't earn) shows before anything moves.
    if (!pinDraft && !tribute && socket.current?.readyState === WebSocket.OPEN) {
      socket.current.send(JSON.stringify(command));
      setDraft(""); setReply(null);
      return;
    }
    setBusy(true);
    const result = await send<{ message: Message }>("POST", path, command);
    if (result.ok) {
      merge([result.data.message]); setDraft(""); setReply(null); setTribute(null);
      if (pinDraft) await changePin(result.data.message.id, reason);
    } else setError(result.error);
    setBusy(false);
  }

  return <section className="chat panel" aria-label="Chat">
    <h2>Chat {mode !== "live" && <span className="muted small">{mode === "polling" ? "(updates every few seconds)" : "(connecting…)"}</span>}</h2>
    {pinned && <aside className="chat-pin" aria-label="Pinned message" aria-live="polite">
      <strong>Pinned message</strong><div className="chat-pin-content"><strong>{pinned.author.display_name}: </strong><MessageBody message={pinned} account={account} emotes={emotes} /></div>
      {canPin && <button type="button" className="small quiet" onClick={() => changePin(null)}>Unpin</button>}
    </aside>}
    {notice && <p className="chat-system" role="status">{notice}</p>}
    {followersOnly && <p className="chat-system" role="status">Followers-only chat until {time(followersOnly)} (followers of at least 10 minutes can chat).{canPin && <> <button type="button" className="small quiet" onClick={() => protect(false)}>End now</button></>}</p>}
    {subsOnly ? <p className="chat-system" role="status">Subscriber-only chat.{canPin && <> <button type="button" className="small quiet" onClick={() => limitToSubs(false)}>Open to everyone</button></>}</p>
      : canPin && <p className="chat-system"><button type="button" className="small quiet" onClick={() => limitToSubs(true)}>Subscriber-only chat</button></p>}
    {canPin && spike && !followersOnly && <div className="chat-system" role="alert">A sudden wave of new viewers arrived. <button type="button" className="small" onClick={() => protect(true)}>Followers-only chat for 10 minutes</button> <button type="button" className="small quiet" onClick={() => setSpike(false)}>Dismiss</button></div>}
    <ol className="chat-messages" ref={list} aria-live="polite">
      {messages.length === 0 && <li className="muted">No messages yet.</li>}
      {messages.map(m => <li key={m.id} className={m.tribute ? "chat-tribute" : undefined}>
        {m.tribute && <span className="badge tribute-badge">{m.tribute.toLocaleString()} Valor</span>}
        <span className="muted">{time(m.created_at)}</span>{" "}
        {m.author.faction && <Crest faction={m.author.faction} size={14} />}{" "}
        {m.author.guild && <GuildChatBadge guild={m.author.guild} />}
        {m.author.username ? <Link className="faction-name" data-faction={m.author.faction} href={`/${m.author.username}`}><strong>{m.author.display_name}</strong></Link> : <strong>{m.author.display_name}</strong>}
        {m.sub && <span className="badge sub-badge" title={`Tier ${m.sub.tier} subscriber`}>{subBadge(m.sub.months)}</span>}
        {m.origin && <span className="badge magnet-badge" title="Sent from MAGNet">MAGNet</span>}
        {m.role && <span className="badge">{{ owner: "Broadcaster", moderator: "Moderator", staff: "Staff" }[m.role]}</span>}: <MessageBody message={m} account={account} emotes={emotes} />
        <details className="chat-message-actions"><summary aria-label={`Actions for message from ${m.author.display_name}`}>Actions</summary><div className="chat-message-controls">
        {account && <button type="button" className="small quiet" aria-label={`Reply to ${m.author.display_name}`} onClick={() => { setReply({ id: m.id, username: m.author.username, body: Array.from(m.body.replace(/[\r\n]+/g, " ")).slice(0, 80).join("") }); input.current?.focus(); }}>Reply</button>}
        {canPin && <button type="button" className="small quiet" aria-label={`Pin message from ${m.author.display_name}`} onClick={() => changePin(m.id)}>Pin</button>}
        {account && m.author.username && m.author.username !== account && <ReportButton target={{ target_type: "chat_message", target_id: m.id }} />}
        {(!account || !m.author.username || m.author.username === account) && <TakeDownLink target={{ target_type: "chat_message", target_id: m.id }} />}
        {role && m.author.username && m.author.username !== account && <span className="chat-actions">
          <button type="button" className="small quiet" onClick={() => moderate("delete", m)} aria-label={`Delete message from ${m.author.display_name}`}>Delete</button>
          <button type="button" className="small quiet" onClick={() => moderate("timeout", m)} aria-label={`Time out ${m.author.display_name}`}>Timeout</button>
          <button type="button" className="small quiet" onClick={() => moderate("ban", m)} aria-label={`Ban ${m.author.display_name}`}>Ban</button>
        </span>}
        </div></details>
      </li>)}
    </ol>
    {squad && role && <details><summary>Shared-chat restrictions ({restrictions.length})</summary><ul className="list">{restrictions.map(r => <li key={`${r.user.username}:${r.kind}`}>{r.user.display_name} · {r.kind}{r.until && ` until ${time(r.until)}`}<button className="small quiet" onClick={async () => { const reason = window.prompt("Reason for lifting this restriction")?.trim(); if (!reason || !r.user.username) return; const result = await send("DELETE", `${path}/restrictions/${encodeURIComponent(r.user.username)}/${r.kind}`, { reason }); if (!result.ok) setError(result.error); else await loadRole(); }}>Lift</button></li>)}</ul></details>}
    <details className="chat-emotes"><summary>Channel emotes ({emotes.length})</summary>
      {emotes.length === 0 ? <p className="muted">No channel emotes yet.</p> : <ul className="list">{emotes.map(emote => <li key={emote.id}>
        {account ? <button type="button" className="quiet small" disabled={busy} onClick={() => { setDraft(value => `${value}${value && !/\s$/.test(value) ? " " : ""}${emote.code} `.slice(0, 500)); input.current?.focus(); }} aria-label={`Insert ${emote.code}`}><EmoteImage emote={emote} /> {emote.code}</button> : <span className="row"><EmoteImage emote={emote} /> {emote.code}</span>}
        {account && account.toLowerCase() !== username.toLowerCase() ? <ReportButton label={`Report ${emote.code}`} target={{ target_type: "emote", target_id: emote.id }} /> : <TakeDownLink target={{ target_type: "emote", target_id: emote.id }} />}
      </li>)}</ul>}
    </details>
    {account ? <form onSubmit={submit} className="chat-form">
      {reply && <div className="chat-reply-draft"><span>Replying to @{reply.username}: {reply.body}</span><button type="button" className="small quiet" onClick={() => setReply(null)}>Cancel reply</button></div>}
      <label htmlFor="chat-input" className="sr-only">Message</label>
      <textarea ref={input} id="chat-input" value={draft} disabled={busy} maxLength={500} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
      <button type="submit" disabled={busy}>{tribute ? "Pay tribute" : "Send"}</button>
      {tribute === null ? <button type="button" className="quiet small" disabled={busy} onClick={() => setTribute(10)}>Tribute</button>
        : <span className="tribute-draft"><label htmlFor="tribute-amount">Valor</label> <input id="tribute-amount" type="number" min={10} step={1} value={tribute} onChange={e => setTribute(Math.max(0, Math.floor(Number(e.target.value) || 0)))} /> <button type="button" className="quiet small" onClick={() => setTribute(null)}>No tribute</button> <Link href="/wallet" className="small">Get Valor</Link></span>}
      {canPin && <button type="button" className="quiet small" disabled={busy || !draft.trim()} onClick={event => submit(event, true)}>Send and pin</button>}
      {error && <p role="alert" className="error">{error}</p>}
    </form> : <p className="muted"><Link href="/login">Sign in</Link> to chat.</p>}
  </section>;
}
