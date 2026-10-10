"use client";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Account = { platform: string; handle: string; enabled: boolean; status: string; detail: string | null };
type Mute = { platform: string; sender_id: string; name: string; permanent: boolean };
type Linked = { accounts: Account[]; mutes: Mute[]; available: Record<string, boolean> };

const NAMES: Record<string, string> = { twitch: "Twitch", youtube: "YouTube", kick: "Kick" };
const STATUS: Record<string, string> = { idle: "Ready (connects when you go live)", connecting: "Connecting…", connected: "Connected", reconnecting: "Reconnecting", revoked: "Needs linking again" };

/** Creator Studio → Linked chat (docs/LINKED_CHAT.md). */
export default function LinkedChatPage() {
  const [data, setData] = useState<Linked | null>(null);
  const [me, setMe] = useState("");
  const [message, setMessage] = useState("");
  const apply = (result: Result<Linked>, saved: string) => {
    if (result.ok) { setData(result.data); setMessage(saved); } else setMessage(result.error);
  };
  const load = useCallback(async () => {
    const [linked, account] = await Promise.all([send<Linked>("GET", "/api/me/linked-chat"), send<{ username: string }>("GET", "/api/auth/me")]);
    if (linked.ok) setData(linked.data); else setMessage(linked.error);
    if (account.ok) setMe(account.data.username);
  }, []);
  useLoad(load);
  async function link(platform: string) {
    const result = await send<{ url: string }>("POST", `/api/auth/oauth/${platform}/start`, { intent: "chat" });
    if (result.ok) window.location.assign(result.data.url); else setMessage(result.error);
  }
  async function unmute(m: Mute) {
    const result = await send("DELETE", `/api/channels/${encodeURIComponent(me)}/chat/outside-mutes/${m.platform}/${encodeURIComponent(m.sender_id)}`);
    if (result.ok) { setMessage(`${m.name} unmuted.`); await load(); } else setMessage(result.error);
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>Linked chat</h1>
    <Section title="Bring your other chats here" intro="Link your accounts on other platforms. While you're live on S.V.E.R, their chat appears in your S.V.E.R chat, each message marked with its platform. You can reply to a message there from S.V.E.R, as your own account on that platform. Outside messages never count toward viewers, Valor, factions or MAGNet.">
      {message && <p role="status" className="form-message">{message}</p>}
      <ul className="list">{Object.keys(NAMES).map(platform => {
        const account = data.accounts.find(a => a.platform === platform);
        return <li key={platform} className="stack">
          <div className="row between wrap">
            <span><span className={`badge platform-badge ${platform}`}>{NAMES[platform]}</span> {account ? <strong>{account.handle}</strong> : <span className="muted">Not linked</span>}</span>
            {account && <span className={`badge${account.status === "connected" ? " verified" : ""}`}>{STATUS[account.status] ?? account.status}</span>}
          </div>
          {account?.detail && account.status !== "idle" && <p className="muted small">{account.detail}</p>}
          <div className="row wrap">
            {account ? <>
              <label className="row"><input type="checkbox" checked={account.enabled} onChange={async e => apply(await send<Linked>("PATCH", `/api/me/linked-chat/${platform}`, { enabled: e.target.checked }), e.target.checked ? "Turned on." : "Turned off.")} /> Show this chat on S.V.E.R</label>
              {account.status === "revoked" && <button type="button" className="small" onClick={() => link(platform)}>Link again</button>}
              <button type="button" className="small quiet danger-text" onClick={async () => apply(await send<Linked>("DELETE", `/api/me/linked-chat/${platform}`), `${NAMES[platform]} unlinked. Its access was removed.`)}>Unlink</button>
            </> : data.available[platform]
              ? <button type="button" className="small" onClick={() => link(platform)}>Link {NAMES[platform]}</button>
              : <span className="muted small">Coming soon: waiting for {NAMES[platform]}&apos;s approval of S.V.E.R&apos;s app.</span>}
          </div>
        </li>;
      })}</ul>
      <p className="muted small">Linking asks {NAMES.twitch} only for permission to read your channel&apos;s chat and post as you. S.V.E.R keeps the access encrypted and deletes it when you unlink. Some platforms don&apos;t allow other platforms&apos; chat inside the stream video, so keep linked chat off the video you send elsewhere; the S.V.E.R chat overlay for OBS leaves outside messages out.</p>
    </Section>
    <Section title="Muted outside chatters" intro="Muting someone only hides them on S.V.E.R; use each platform's own tools to moderate there.">
      {data.mutes.length === 0 ? <p className="muted">Nobody is muted.</p>
        : <ul className="list">{data.mutes.map(m => <li key={`${m.platform}:${m.sender_id}`} className="row between wrap">
          <span><span className={`badge platform-badge ${m.platform}`}>{NAMES[m.platform] ?? m.platform}</span> {m.name} <span className="muted small">{m.permanent ? "permanently" : "for this stream"}</span></span>
          <button type="button" className="small quiet" disabled={!me} onClick={() => unmute(m)}>Unmute</button>
        </li>)}</ul>}
    </Section>
  </>;
}
