"use client";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Avatar } from "../../components/Avatar";
import { ReportButton } from "../../components/Report";
import { ArrowIcon, LockIcon, MoreIcon, MutedIcon } from "../../components/shell/Icons";
import { PlayerCardButton } from "../../components/UserChip";
import { send, useLoad } from "../../lib/client-api";
import { UsernameInput } from "../../components/UsernameInput";
import { open as openEnvelope, safetyNumber, seal, type Device, type Envelope, type Opened } from "../../lib/e2e";
import { ensureDevice, type ThisDevice } from "../../lib/e2e-device";
import type { Chip } from "../../lib/types";

type Message = { id: string; seq: number; sender: string; body: string | null; created_at: string; private?: boolean; envelope?: Envelope; commitment?: string };
type Conversation = { with: Chip; last: string; last_at: string; unread: number; muted: boolean };
type Thread = {
  with: Chip; messages: Message[]; can_send: boolean; reason: string | null; muted: boolean; private: boolean;
  private_changed: { at: string; by: string | null } | null; their_device_at: string | null;
};
type MyDevice = { id: string; created_at: string };

const LINK = /https?:\/\/|www\./i;
/** "now", "5m", "3h", "2d", then a date: for the conversation list. */
function ago(iso: string) {
  const minutes = Math.floor((Date.now() - Date.parse(iso)) / 60000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)}h`;
  if (minutes < 10080) return `${Math.floor(minutes / 1440)}d`;
  return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
const time = (iso: string) => new Date(iso).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
/** A page of a conversation (dms.rs PAGE): a full page means there may be older ones. */
const PAGE = 50;

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
  const [devices, setDevices] = useState<MyDevice[]>([]);
  const [safety, setSafety] = useState("");
  const [panel, setPanel] = useState<"new" | "settings" | null>(null);
  const [sending, setSending] = useState(false);
  // Older messages: whether there may be more, and whether a page is loading.
  const [more, setMore] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  // The message whose time and Report are shown (the last of each run always shows them).
  const [shown, setShown] = useState<string | null>(null);
  const opened = useRef<string | null>(null);

  const loadList = useCallback(async () => {
    const r = await send<{ conversations: Conversation[] }>("GET", "/api/dms");
    if (r.ok) setList(r.data.conversations); else setError(r.error);
  }, []);
  const loadDevices = useCallback(async () => {
    const r = await send<{ devices: MyDevice[] }>("GET", "/api/me/dm-devices");
    if (r.ok) setDevices(r.data.devices);
  }, []);
  const loadThread = useCallback(async (name: string) => {
    const r = await send<Thread>("GET", `/api/dms/${encodeURIComponent(name)}`);
    if (!r.ok) { setError(r.error); return; }
    setThread(r.data);
    setMore(r.data.messages.length >= PAGE);
    // Recomputed on every load, so a device change never leaves an old number showing.
    const keys = r.data.private ? await send<{ mine: Device[]; theirs: Device[] }>("GET", `/api/dms/${encodeURIComponent(name)}/keys`) : null;
    setSafety(keys?.ok ? await safetyNumber(keys.data.mine, keys.data.theirs) : "");
    if (r.data.messages.length) await send("POST", `/api/dms/${encodeURIComponent(name)}/read`);
    await loadList();
  }, [loadList]);
  const loadAll = useCallback(async () => {
    const [account, settings] = await Promise.all([send<{ id: string; username: string }>("GET", "/api/auth/me"), send<{ policy: string }>("GET", "/api/me/dm-settings")]);
    if (account.ok) {
      setMe(account.data.username);
      const mine = await ensureDevice(account.data.id);
      if (mine && typeof mine !== "string") setDevice(mine);
      else setDeviceNote(typeof mine === "string" ? mine : "This browser can't keep the keys Private mode needs.");
      await loadDevices();
    }
    if (settings.ok) setPolicy(settings.data.policy);
    await loadList();
    const name = new URLSearchParams(window.location.search).get("with");
    if (name) { setOpen(name); opened.current = name; await loadThread(name); }
  }, [loadList, loadThread, loadDevices]);
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
        // The socket fell behind (resync) or Private mode changed: reload the open conversation too.
        const here = !!opened.current && data.with?.toLowerCase() === opened.current.toLowerCase();
        if (opened.current && (data.type === "resync" || (data.type === "private" && here))) void loadThread(opened.current);
        if (data.type === "dm" && data.message && data.with?.toLowerCase() === opened.current?.toLowerCase()) {
          const message = data.message;
          setThread(t => t && !t.messages.some(m => m.id === message.id) ? { ...t, messages: [...t.messages, message] } : t);
          if (opened.current) void send("POST", `/api/dms/${encodeURIComponent(opened.current)}/read`);
        }
        void loadList();
      };
      // 4001: the session ended (signed out everywhere, password changed); don't keep retrying.
      socket.onclose = event => { if (!closed && event.code !== 4001) setTimeout(connect, 5000); };
    };
    connect();
    return () => { closed = true; clearInterval(poll); socket?.close(); };
  }, [loadList, loadThread]);
  // Follow the newest message; loading older ones above doesn't jump to the bottom.
  const newest = thread?.messages.at(-1)?.id;
  useEffect(() => { bottom.current?.scrollIntoView({ block: "nearest" }); }, [newest]);
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
    setOpen(name); opened.current = name; setError(""); setDraft(""); setSafety(""); setShown(null);
    history.replaceState(null, "", `/messages?with=${encodeURIComponent(name)}`);
    await loadThread(name);
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    // One send at a time: a second Enter while the first is on its way would send it twice.
    if (!open || !draft.trim() || sending) return;
    let payload: object = { body: draft };
    // After Private mode was turned off, the first plain message is a deliberate choice.
    const last = thread?.messages.at(-1);
    if (thread && !thread.private && last?.private && !confirm("Private mode is off. This message won't be end-to-end encrypted. Send it anyway?")) return;
    setSending(true);
    try {
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
    } finally {
      setSending(false);
    }
  }
  async function loadOlder() {
    const first = thread?.messages[0];
    if (!open || !first || loadingOlder) return;
    setLoadingOlder(true);
    const r = await send<Thread>("GET", `/api/dms/${encodeURIComponent(open)}?before=${first.seq}`);
    setLoadingOlder(false);
    if (!r.ok) { setError(r.error); return; }
    setMore(r.data.messages.length >= PAGE);
    setThread(t => t ? { ...t, messages: [...r.data.messages.filter(m => !t.messages.some(x => x.id === m.id)), ...t.messages] } : t);
  }
  async function togglePrivate() {
    if (!open || !thread) return;
    const r = await send("PUT", `/api/dms/${encodeURIComponent(open)}/private`, { on: !thread.private });
    if (!r.ok) { setError(r.error); return; }
    setError("");
    await loadThread(open);
  }
  async function removeDevice(id: string) {
    if (!confirm(id === device?.id ? "Remove this browser? It won't be able to read private messages any more." : "Remove this device? It won't be able to read new private messages.")) return;
    const r = await send("DELETE", `/api/me/dm-devices/${encodeURIComponent(id)}`);
    if (!r.ok) { setError(r.error); return; }
    if (id === device?.id) setDevice(null);
    await loadDevices();
  }
  async function action(kind: "mute" | "clear" | "block") {
    if (!open || !thread) return;
    const who = `@${thread.with.username ?? open}`;
    if (kind === "clear" && !confirm(`Delete this conversation for you? You won't see these messages again. ${who} still will.`)) return;
    if (kind === "block" && !confirm(`Block ${who}? Neither of you can message the other, and the conversation is hidden for both of you. You can unblock in Settings → Blocked.`)) return;
    const name = encodeURIComponent(open);
    const r = kind === "mute" ? await send("PUT", `/api/dms/${name}/mute`, { muted: !thread.muted })
      : kind === "clear" ? await send("DELETE", `/api/dms/${name}`)
      : await send("PUT", `/api/blocks/${name}`);
    if (!r.ok) { setError(r.error); return; }
    await loadThread(open);
  }

  function back() {
    setOpen(null); opened.current = null; setThread(null); setError("");
    history.replaceState(null, "", "/messages");
  }
  const mine = (m: Message) => m.sender.toLowerCase() === me.toLowerCase();

  return <div className="messages-page">
    <div className={`messages-shell panel${thread ? " has-thread" : ""}`}>
      <aside className="messages-sidebar" aria-label="Conversations">
        <header className="messages-sidebar-head">
          <h1>Messages</h1>
          <button type="button" className="small quiet" aria-expanded={panel === "settings"} onClick={() => setPanel(panel === "settings" ? null : "settings")}>Settings</button>
          <button type="button" className="small" aria-expanded={panel === "new"} onClick={() => setPanel(panel === "new" ? null : "new")}>New</button>
        </header>
        {panel === "new" && <form className="messages-new" onSubmit={e => { e.preventDefault(); const name = String(new FormData(e.currentTarget).get("to") ?? "").trim().replace(/^@/, ""); if (name) { setPanel(null); void choose(name); } }}>
          <UsernameInput label="Message @username" name="to" />
          <button type="submit" className="small">Open</button>
          <p className="small-text">The Message button on a channel or player card opens a conversation too.</p>
        </form>}
        {panel === "settings" && <div className="messages-settings">
          <label className="field"><span>Who can message you</span><select value={policy} onChange={async e => { const r = await send<{ policy: string }>("PUT", "/api/me/dm-settings", { policy: e.target.value }); if (r.ok) setPolicy(r.data.policy); else setError(r.error); }}>
            <option value="mutuals">People who follow you and you follow back</option>
            <option value="following">Anyone you follow</option>
            <option value="nobody">Nobody</option>
          </select></label>
          <p className="small-text">Messages are text only and are kept for 12 months. S.V.E.R stores them encrypted, and staff can read a conversation only when it&apos;s reported. Turn on <strong>Private</strong> in a conversation to encrypt it end to end on your devices: nobody else, including S.V.E.R, can read it, and a report sends only the message you report. Private messages can&apos;t be read on a device added later. Private mode is for two adults.</p>
          {devices.length > 0 && <div><strong className="small-text">Your Private-mode devices</strong>
            <ul className="messages-devices">{devices.map(d => <li key={d.id}>Added {time(d.created_at)}{d.id === device?.id && <strong> (this browser)</strong>} <button type="button" className="small quiet danger-text" onClick={() => void removeDevice(d.id)}>Remove</button></li>)}</ul>
          </div>}
        </div>}
        {list === null ? <p className="loading messages-pad">Loading…</p> : list.length === 0 ? <p className="muted messages-pad">No conversations yet. Press <strong>New</strong>, or use Message on someone&apos;s channel or player card.</p>
          : <ul className="messages-list">{list.map(c => <li key={c.with.username}><button type="button" className={`messages-item${c.with.username?.toLowerCase() === open?.toLowerCase() ? " active" : ""}${c.unread > 0 && !c.muted ? " unread" : ""}`} onClick={() => c.with.username && choose(c.with.username)}>
            <Avatar sizes={c.with.avatar} name={c.with.display_name} size={40} />
            <span className="messages-item-text">
              <span className="messages-item-top"><strong>{c.with.display_name}</strong><span className="small-text">{ago(c.last_at)}</span></span>
              <span className="small-text">{c.muted && <span className="messages-muted" title="Muted"><MutedIcon size={12} /><span className="sr-only">Muted: </span></span>}{c.last}</span>
            </span>
            {c.unread > 0 && !c.muted && <span className="messages-unread">{c.unread}<span className="sr-only"> unread</span></span>}
          </button></li>)}</ul>}
      </aside>
      <section className="messages-thread" aria-label="Conversation">
        {!thread ? <div className="messages-empty"><p>Choose a conversation, or start a new one.</p><button type="button" onClick={() => setPanel("new")}>New message</button>{error && <p role="alert" className="form-message error">{error}</p>}</div> : <>
          <header className="messages-thread-head">
            <button type="button" className="small quiet messages-back" onClick={back} aria-label="Back to conversations"><ArrowIcon dir="left" /></button>
            <Avatar sizes={thread.with.avatar} name={thread.with.display_name} size={36} />
            {thread.with.username ? <PlayerCardButton username={thread.with.username}><span><strong>{thread.with.display_name}</strong> <span className="handle">@{thread.with.username}</span></span></PlayerCardButton> : <strong>{thread.with.display_name}</strong>}
            <span className="grow" />
            <button type="button" className={`small${thread.private ? "" : " quiet"}`} aria-pressed={thread.private} disabled={!device && !thread.private} title={device ? "Private mode: end-to-end encrypted on your devices" : deviceNote} onClick={() => void togglePrivate()}>{thread.private ? <><LockIcon /> Private</> : "Private: off"}</button>
            <details className="menu messages-more"><summary className="button small quiet" aria-label="More actions"><MoreIcon /></summary>
              <div className="menu-list panel" role="menu">
                {(["mute", "clear", "block"] as const).map(kind => <button key={kind} type="button" role="menuitem" className={`link-button${kind === "block" ? " danger-text" : ""}`} onClick={e => { e.currentTarget.closest("details")?.removeAttribute("open"); void action(kind); }}>{kind === "mute" ? thread.muted ? "Unmute" : "Mute" : kind === "clear" ? "Delete for me" : "Block"}</button>)}
              </div>
            </details>
          </header>
          {(thread.private_changed || thread.private) && <div className="messages-notices">
            {thread.private_changed && <p className="small-text" role="status">{thread.private_changed.by ? `@${thread.private_changed.by}` : "Someone"} turned Private mode {thread.private ? "on" : "off"} {time(thread.private_changed.at)}.{!thread.private && " New messages aren't end-to-end encrypted."}</p>}
            {thread.private && thread.their_device_at && thread.private_changed && Date.parse(thread.their_device_at) > Date.parse(thread.private_changed.at) && <p className="small-text" role="status">@{thread.with.username} added a device {time(thread.their_device_at)}. New private messages can be read there too.</p>}
            {thread.private && <details className="small-text">
              <summary>Safety number</summary>
              <p><code>{safety || "Couldn't load the keys."}</code></p>
              <p className="muted">Compare this with @{thread.with.username} in person or over another app. If both of you see the same number, your messages are going only to each other&apos;s devices. It changes when either of you adds or removes a device.</p>
            </details>}
          </div>}
          <ol className="messages-body" aria-live="polite">
            {more && <li className="messages-start"><button type="button" className="small quiet" disabled={loadingOlder} onClick={() => void loadOlder()}>{loadingOlder ? "Loading…" : "Load older messages"}</button></li>}
            {thread.messages.length === 0 && <li className="messages-start small-text">This is the start of your conversation with @{thread.with.username}.</li>}
            {thread.messages.map((m, i) => {
              const next = thread.messages[i + 1];
              // The time goes under the last of a run from one person within 5 minutes; any other
              // message shows its time and Report when you tap or press it.
              const last = !next || next.sender !== m.sender || Date.parse(next.created_at) - Date.parse(m.created_at) > 300000;
              const reveal = last || shown === m.id;
              const toggle = () => { if (!last) setShown(s => s === m.id ? null : m.id); };
              return <li key={m.id} className={`${mine(m) ? "mine" : "theirs"}${last ? " run-end" : ""}`}>
                <span className="messages-text" {...(last ? {} : { role: "button", tabIndex: 0, "aria-expanded": shown === m.id, title: "Show the time and Report", onClick: toggle, onKeyDown: (e: React.KeyboardEvent) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } } })}>{m.private ? <><span className="messages-lock" title="Private"><LockIcon size={12} /><span className="sr-only">Private: </span></span>{plain[m.id]?.body ?? (m.id in plain ? <em className="muted">This message can&apos;t be read on this device.</em> : "…")}</> : m.body}</span>
                {reveal && <span className="messages-meta small-text">{time(m.created_at)}{!mine(m) && (!m.private || m.id in plain) && <> · <ReportButton target={{ target_type: "dm_message", target_id: m.id, ...(m.private && plain[m.id] ? { evidence: { body: plain[m.id]!.body, frank: plain[m.id]!.frank } } : {}) }} /></>}</span>}
              </li>;
            })}
            <li ref={bottom} aria-hidden="true" />
          </ol>
          {error && <p role="alert" className="form-message error messages-pad">{error}</p>}
          {thread.can_send ? <form onSubmit={submit} className="messages-composer">
            {LINK.test(draft) && <p className="small-text messages-link-note" role="status">Links can lead anywhere. Only send ones you trust, and be careful opening links you receive.</p>}
            <label htmlFor="dm-input" className="sr-only">Message</label>
            {/* Enter sends with a keyboard; touch keyboards have no Shift+Enter, so there Enter is a new line. */}
            <textarea id="dm-input" value={draft} placeholder={thread.private ? "Private message" : `Message @${thread.with.username}`} onChange={e => setDraft(e.target.value)} maxLength={1000} rows={1} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && !window.matchMedia("(pointer: coarse)").matches) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
            <button type="submit" disabled={!draft.trim() || sending}>{sending ? "Sending…" : "Send"}</button>
          </form> : <p className="muted messages-composer" role="status">{thread.reason}</p>}
        </>}
      </section>
    </div>
  </div>;
}
