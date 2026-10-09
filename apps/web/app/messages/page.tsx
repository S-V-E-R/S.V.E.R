"use client";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Avatar } from "../../components/Avatar";
import { ReportButton } from "../../components/Report";
import { send, useLoad } from "../../lib/client-api";
import type { Chip } from "../../lib/types";

type Message = { id: string; seq: number; sender: string; body: string; created_at: string };
type Conversation = { with: Chip; last: string; last_at: string; unread: number; muted: boolean };
type Thread = { with: Chip; messages: Message[]; can_send: boolean; reason: string | null; muted: boolean };

const LINK = /https?:\/\/|www\./i;
const time = (iso: string) => new Date(iso).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

/** Direct messages (docs/COMMUNITY.md "Direct messages"). `?with=username` opens a conversation. */
export default function Messages() {
  const [list, setList] = useState<Conversation[] | null>(null);
  const [me, setMe] = useState("");
  const [policy, setPolicy] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [thread, setThread] = useState<Thread | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const bottom = useRef<HTMLLIElement>(null);
  const opened = useRef<string | null>(null);

  const loadList = useCallback(async () => {
    const r = await send<{ conversations: Conversation[] }>("GET", "/api/dms");
    if (r.ok) setList(r.data.conversations); else setError(r.error);
  }, []);
  const loadThread = useCallback(async (name: string) => {
    const r = await send<Thread>("GET", `/api/dms/${encodeURIComponent(name)}`);
    if (!r.ok) { setError(r.error); return; }
    setThread(r.data);
    if (r.data.messages.length) await send("POST", `/api/dms/${encodeURIComponent(name)}/read`);
    await loadList();
  }, [loadList]);
  const loadAll = useCallback(async () => {
    const [account, settings] = await Promise.all([send<{ username: string }>("GET", "/api/auth/me"), send<{ policy: string }>("GET", "/api/me/dm-settings")]);
    if (account.ok) setMe(account.data.username);
    if (settings.ok) setPolicy(settings.data.policy);
    await loadList();
    const name = new URLSearchParams(window.location.search).get("with");
    if (name) { setOpen(name); opened.current = name; await loadThread(name); }
  }, [loadList, loadThread]);
  useLoad(loadAll);

  // New messages arrive over a socket; while it's down, the page reloads every 15 seconds.
  useEffect(() => {
    let closed = false;
    let socket: WebSocket | null = null;
    const poll = setInterval(() => { if (!socket || socket.readyState !== WebSocket.OPEN) { void loadList(); if (opened.current) void loadThread(opened.current); } }, 15000);
    const connect = () => {
      socket = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/dms/socket`);
      socket.onmessage = event => {
        const data = JSON.parse(event.data) as { type: string; with?: string; message?: Message };
        if (data.type === "dm" && data.message && data.with?.toLowerCase() === opened.current?.toLowerCase()) {
          const message = data.message;
          setThread(t => t && !t.messages.some(m => m.id === message.id) ? { ...t, messages: [...t.messages, message] } : t);
          if (opened.current) void send("POST", `/api/dms/${encodeURIComponent(opened.current)}/read`);
        }
        void loadList();
      };
      socket.onclose = () => { if (!closed) setTimeout(connect, 5000); };
    };
    connect();
    return () => { closed = true; clearInterval(poll); socket?.close(); };
  }, [loadList, loadThread]);
  useEffect(() => { bottom.current?.scrollIntoView({ block: "nearest" }); }, [thread?.messages.length]);

  async function choose(name: string) {
    setOpen(name); opened.current = name; setError(""); setDraft("");
    history.replaceState(null, "", `/messages?with=${encodeURIComponent(name)}`);
    await loadThread(name);
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!open || !draft.trim()) return;
    const r = await send<{ message: Message }>("POST", `/api/dms/${encodeURIComponent(open)}`, { body: draft });
    if (!r.ok) { setError(r.error); return; }
    setDraft(""); setError("");
    setThread(t => t && !t.messages.some(m => m.id === r.data.message.id) ? { ...t, messages: [...t.messages, r.data.message] } : t);
    await loadList();
  }
  async function action(kind: "mute" | "clear" | "block") {
    if (!open || !thread) return;
    const name = encodeURIComponent(open);
    const r = kind === "mute" ? await send("PUT", `/api/dms/${name}/mute`, { muted: !thread.muted })
      : kind === "clear" ? await send("DELETE", `/api/dms/${name}`)
      : await send("PUT", `/api/blocks/${name}`);
    if (!r.ok) { setError(r.error); return; }
    await loadThread(open);
  }

  return <div className="messages-page"><h1>Messages</h1>
    <div className="row wrap">
      <label className="field narrow"><span>Who can message you</span><select value={policy} onChange={async e => { const r = await send<{ policy: string }>("PUT", "/api/me/dm-settings", { policy: e.target.value }); if (r.ok) setPolicy(r.data.policy); else setError(r.error); }}>
        <option value="mutuals">People who follow you and you follow back</option>
        <option value="following">Anyone you follow</option>
        <option value="nobody">Nobody</option>
      </select></label>
      <form className="row" onSubmit={e => { e.preventDefault(); const name = String(new FormData(e.currentTarget).get("to") ?? "").trim().replace(/^@/, ""); if (name) void choose(name); }}>
        <label className="field narrow"><span>New message to</span><input name="to" placeholder="username" autoComplete="off" maxLength={26} /></label>
        <button type="submit" className="small">Open</button>
      </form>
    </div>
    <p className="muted small">Messages are text only and are kept for 12 months. They aren&apos;t end-to-end encrypted: S.V.E.R stores them encrypted and staff can read a conversation only when it&apos;s reported.</p>
    {error && <p role="alert" className="form-message error">{error}</p>}
    <div className="messages-layout">
      <nav className="panel messages-list" aria-label="Conversations">
        {list === null ? <p className="loading">Loading…</p> : list.length === 0 ? <p className="muted">No conversations yet.</p>
          : <ul>{list.map(c => <li key={c.with.username}><button type="button" className={`messages-item${c.with.username?.toLowerCase() === open?.toLowerCase() ? " active" : ""}`} onClick={() => c.with.username && choose(c.with.username)}>
            <Avatar sizes={c.with.avatar} name={c.with.display_name} size={32} />
            <span className="messages-item-text"><strong>{c.with.display_name}</strong><span className="muted small">{c.last}</span></span>
            {c.unread > 0 && !c.muted && <span className="badge verified">{c.unread}<span className="sr-only"> unread</span></span>}
          </button></li>)}</ul>}
      </nav>
      <section className="panel messages-thread" aria-label="Conversation">
        {!thread ? <p className="muted">Choose a conversation.</p> : <>
          <header className="row between wrap">
            <Link href={`/${thread.with.username}`}><strong>{thread.with.display_name}</strong> <span className="muted">@{thread.with.username}</span></Link>
            <span className="row wrap">
              <button type="button" className="small quiet" onClick={() => action("mute")}>{thread.muted ? "Unmute" : "Mute"}</button>
              <button type="button" className="small quiet" onClick={() => action("clear")}>Delete for me</button>
              <button type="button" className="small quiet danger-text" onClick={() => action("block")}>Block</button>
            </span>
          </header>
          <ol className="messages-body">{thread.messages.map(m => <li key={m.id} className={m.sender.toLowerCase() === me.toLowerCase() ? "mine" : "theirs"}>
            <span className="messages-text">{m.body}</span>
            <span className="muted small">{time(m.created_at)}{m.sender.toLowerCase() !== me.toLowerCase() && <> · <ReportButton target={{ target_type: "dm_message", target_id: m.id }} /></>}</span>
          </li>)}<li ref={bottom} aria-hidden="true" /></ol>
          {thread.can_send ? <form onSubmit={submit} className="stack">
            <label htmlFor="dm-input" className="sr-only">Message</label>
            <textarea id="dm-input" value={draft} onChange={e => setDraft(e.target.value)} maxLength={1000} rows={2} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
            {LINK.test(draft) && <p className="muted small" role="status">Links can lead anywhere. Only send ones you trust, and be careful opening links you receive.</p>}
            <button type="submit" disabled={!draft.trim()}>Send</button>
          </form> : <p className="muted" role="status">{thread.reason}</p>}
        </>}
      </section>
    </div>
  </div>;
}
