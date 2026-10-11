"use client";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Avatar } from "../../components/Avatar";
import { ReportButton } from "../../components/Report";
import { send, useLoad } from "../../lib/client-api";
import { open as openEnvelope, seal, type Device, type Envelope, type Opened } from "../../lib/e2e";
import { ensureDevice, type ThisDevice } from "../../lib/e2e-device";
import type { Chip } from "../../lib/types";

type Message = { id: string; seq: number; sender: string; body: string | null; created_at: string; private?: boolean; envelope?: Envelope; commitment?: string };
type Conversation = { with: Chip; last: string; last_at: string; unread: number; muted: boolean };
type Thread = { with: Chip; messages: Message[]; can_send: boolean; reason: string | null; muted: boolean; private: boolean };

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
  // Private mode: this browser's device key, and what it could decrypt (null: not readable here).
  const [device, setDevice] = useState<ThisDevice | null>(null);
  const [deviceNote, setDeviceNote] = useState("");
  const [plain, setPlain] = useState<Record<string, Opened | null>>({});
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
    if (account.ok) {
      setMe(account.data.username);
      const mine = await ensureDevice(account.data.username);
      if (mine && typeof mine !== "string") setDevice(mine);
      else setDeviceNote(typeof mine === "string" ? mine : "This browser can't keep the keys Private mode needs.");
    }
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
        if (data.type === "private" && data.with?.toLowerCase() === opened.current?.toLowerCase() && opened.current) void loadThread(opened.current);
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
  // Decrypt private messages on this device as they arrive.
  useEffect(() => {
    if (!device || !thread) return;
    const todo = thread.messages.filter(m => m.private && m.envelope && m.commitment && !(m.id in plain));
    if (!todo.length) return;
    let live = true;
    void Promise.all(todo.map(async m => [m.id, await openEnvelope(m.envelope!, m.commitment!, device.id, device.privateKey)] as const))
      .then(done => { if (live) setPlain(p => ({ ...p, ...Object.fromEntries(done) })); });
    return () => { live = false; };
  }, [device, thread, plain]);

  async function choose(name: string) {
    setOpen(name); opened.current = name; setError(""); setDraft("");
    history.replaceState(null, "", `/messages?with=${encodeURIComponent(name)}`);
    await loadThread(name);
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!open || !draft.trim()) return;
    let payload: object = { body: draft };
    if (thread?.private) {
      // Encrypted here for both people's devices; the server never sees the text.
      if (!device) { setError(deviceNote || "This browser can't send private messages."); return; }
      const keys = await send<{ mine: Device[]; theirs: Device[] }>("GET", `/api/dms/${encodeURIComponent(open)}/keys`);
      if (!keys.ok) { setError(keys.error); return; }
      payload = await seal(draft.trim(), [...keys.data.mine, ...keys.data.theirs]);
    }
    const r = await send<{ message: Message }>("POST", `/api/dms/${encodeURIComponent(open)}`, payload);
    if (!r.ok) { setError(r.error); return; }
    setDraft(""); setError("");
    setThread(t => t && !t.messages.some(m => m.id === r.data.message.id) ? { ...t, messages: [...t.messages, r.data.message] } : t);
    await loadList();
  }
  async function togglePrivate() {
    if (!open || !thread) return;
    const r = await send("PUT", `/api/dms/${encodeURIComponent(open)}/private`, { on: !thread.private });
    if (!r.ok) { setError(r.error); return; }
    setError("");
    await loadThread(open);
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
    <p className="muted small">Messages are text only and are kept for 12 months. Normally S.V.E.R stores them encrypted and staff can read a conversation only when it&apos;s reported. Turn on <strong>Private</strong> in a conversation to encrypt it end to end on your devices: nobody else, including S.V.E.R, can read it; a report sends only the message you report. Private messages can&apos;t be read on a device added later.</p>
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
              <button type="button" className="small quiet" aria-pressed={thread.private} disabled={!device && !thread.private} title={device ? "Private mode: end-to-end encrypted on your devices" : deviceNote} onClick={() => void togglePrivate()}>{thread.private ? "🔒 Private: on" : "Private: off"}</button>
              <button type="button" className="small quiet" onClick={() => action("mute")}>{thread.muted ? "Unmute" : "Mute"}</button>
              <button type="button" className="small quiet" onClick={() => action("clear")}>Delete for me</button>
              <button type="button" className="small quiet danger-text" onClick={() => action("block")}>Block</button>
            </span>
          </header>
          <ol className="messages-body">{thread.messages.map(m => <li key={m.id} className={m.sender.toLowerCase() === me.toLowerCase() ? "mine" : "theirs"}>
            <span className="messages-text">{m.private ? <><span aria-label="Private">🔒 </span>{plain[m.id]?.body ?? (m.id in plain ? <em className="muted">This message can&apos;t be read on this device.</em> : "…")}{plain[m.id] && !plain[m.id]!.verified && <em className="muted small"> (couldn&apos;t be verified)</em>}</> : m.body}</span>
            <span className="muted small">{time(m.created_at)}{m.sender.toLowerCase() !== me.toLowerCase() && (!m.private || plain[m.id]) && <> · <ReportButton target={{ target_type: "dm_message", target_id: m.id, ...(m.private && plain[m.id] ? { evidence: { body: plain[m.id]!.body, frank: plain[m.id]!.frank } } : {}) }} /></>}</span>
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
