"use client";
import Link from "next/link";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { LivePlayer } from "./LivePlayer";
import { Chat } from "./Chat";
import { Crest } from "./FactionIdentity";

export type Squad = { id: string; mode: "SEPARATE" | "MERGED"; ended: boolean; members: (Chip & { host: boolean })[]; host: boolean; joined: boolean; invited: boolean; pending: { username: string; expires_at: string }[] };
export type MySquads = { current: string | null; live: boolean; invites: { id: string; host: string; mode: string; expires_at: string }[] };
export function SquadStudio({ initial }: { initial: MySquads }) {
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const router = useRouter();
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true);
    const r = await send<{ id: string }>("POST", "/api/me/squads", { mode: new FormData(event.currentTarget).get("mode") });
    if (r.ok) router.push(`/squads/${r.data.id}`); else setNotice(r.error);
    setBusy(false);
  }
  return <><h1>Co-streams</h1><p>Invite up to three live streamers and watch together. Each channel keeps its own stream and viewer count.</p>
    {initial.current ? <p><Link className="button" href={`/squads/${initial.current}`}>Open your co-stream</Link></p> : <section className="panel section"><h2>Start a co-stream</h2>{initial.live ? <form onSubmit={create}><fieldset><legend>Chat mode</legend><label className="checkbox"><input type="radio" name="mode" value="SEPARATE" defaultChecked /> Separate — viewers choose a channel&apos;s chat</label><label className="checkbox"><input type="radio" name="mode" value="MERGED" /> Merged — a shared room moderated by every member&apos;s moderators</label></fieldset><button disabled={busy}>Create co-stream</button></form> : <p><Link href="/studio/stream">Go live</Link> before creating or joining a co-stream.</p>}</section>}
    <section className="panel section"><h2>Invitations</h2>{initial.invites.length === 0 ? <p className="muted">No pending invitations.</p> : <ul className="list">{initial.invites.map(i => <li key={i.id}><Link href={`/squads/${i.id}`}>{i.host}&apos;s co-stream</Link> · {i.mode.toLowerCase()} chat<p className="small muted">Expires {new Date(i.expires_at).toLocaleTimeString()}</p></li>)}</ul>}</section>
    <p><Link href="/studio/guilds">Guildmates &amp; team shortcuts</Link></p>{notice && <p role="alert" className="error">{notice}</p>}
  </>;
}
export function SquadView({ initial, account }: { initial: Squad; account: string | null }) {
  const [squad, setSquad] = useState(initial);
  const [audio, setAudio] = useState<string | null>(null);
  const [chat, setChat] = useState(initial.members[0]?.username ?? "");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const path = `/api/squads/${squad.id}`;
  const load = useCallback(async () => { const r = await send<Squad>("GET", path); if (r.ok) setSquad(r.data); else setNotice(r.error); }, [path]);
  useEffect(() => { const timer = setInterval(() => { if (!document.hidden) void load(); }, 5000); return () => clearInterval(timer); }, [load]);
  async function act(action: string, body?: unknown, method = "POST") {
    setBusy(true); const r = await send(method, `${path}/${action}`, body); setNotice(r.ok ? "Saved." : r.error); if (r.ok) await load(); setBusy(false);
  }
  async function invite(event: FormEvent<HTMLFormElement>) { event.preventDefault(); await act("invites", { username: new FormData(event.currentTarget).get("username") }); }
  const chosen = squad.members.find(m => m.username === chat)?.username ?? squad.members[0]?.username;
  const host = squad.members.find(m => m.host)?.username;
  return <div className="teams-page"><header><span className="eyebrow">Live together</span><h1>{host ? `${host}'s co-stream` : "Co-stream"}</h1><p>{squad.mode === "MERGED" ? "One shared chat. Every member's channel bans apply here." : "Choose a channel's chat below."} Select a stream to hear its audio.</p><div className="row"><Link href="/studio/squads">My co-streams</Link>{squad.joined && <button className="small quiet" disabled={busy} onClick={() => { if (!squad.host || window.confirm("End this co-stream for everyone?")) void act("leave"); }}>{squad.host ? "End co-stream" : "Leave co-stream"}</button>}<button className="small quiet" aria-pressed={audio === null} onClick={() => setAudio(null)}>Mute all</button></div></header>
    {notice && <p role="status" className="form-message">{notice}</p>}
    {squad.ended ? <section className="panel section"><h2>This co-stream has ended</h2><p><Link href="/">Find a live stream</Link></p></section> : <>
      {squad.invited && <section className="panel section"><h2>You&apos;re invited</h2><p>Joining adds your live stream to this page with {squad.mode.toLowerCase()} chat.</p><div className="row"><button disabled={busy} onClick={() => act("answer", { accept: true })}>Accept invitation</button><button disabled={busy} className="quiet" onClick={() => act("answer", { accept: false })}>Decline</button></div></section>}
      {squad.host && <details className="panel section" open={squad.members.length < 2}><summary>Invite streamers · {squad.members.length} of 4 streams</summary><form onSubmit={invite}><label className="field">Live streamer username<input name="username" required maxLength={25} /></label><button disabled={busy || squad.members.length + squad.pending.length >= 4}>Send invitation</button></form>{squad.pending.map(p => <div className="row" key={p.username}><span>{p.username} · expires {new Date(p.expires_at).toLocaleTimeString()}</span><button className="quiet small" disabled={busy} onClick={() => act(`invites/${encodeURIComponent(p.username)}`, undefined, "DELETE")}>Cancel invitation</button></div>)}</details>}
      <div className="squad-layout section"><div className="squad-streams">{squad.members.map(m => m.username && <section key={m.username} className="squad-stream panel"><div className="row">{m.faction && <Crest faction={m.faction} size={24} />}<h2><Link href={`/${m.username}/live`}>{m.display_name}</Link>{m.host && " · Host"}</h2></div><LivePlayer username={m.username} signedIn={!!account} nested focused followRaids={false} muted={audio !== m.username} onUnmute={() => setAudio(m.username)} /><button className="quiet small" aria-pressed={audio === m.username} onClick={() => setAudio(audio === m.username ? null : m.username)}>{audio === m.username ? "Mute audio" : `Listen to ${m.display_name}`}</button></section>)}</div>
      <aside className="squad-chat">{squad.mode === "SEPARATE" && <label className="field">Channel chat<select value={chosen ?? ""} onChange={e => setChat(e.target.value)}>{squad.members.map(m => m.username && <option key={m.username} value={m.username}>{m.display_name}</option>)}</select></label>}{squad.mode === "MERGED" ? host && <Chat key={squad.id} username={host} squad={squad.id} account={account} /> : chosen && <Chat key={chosen} username={chosen} account={account} />}</aside></div>
    </>}
  </div>;
}
