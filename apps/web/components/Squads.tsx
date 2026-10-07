"use client";
import Link from "next/link";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { send } from "../lib/client-api";
import type { Chip } from "../lib/types";
import { LivePlayer } from "./LivePlayer";
import { Chat } from "./Chat";
import { Subscribe } from "./Subscribe";
import { Crest } from "./FactionIdentity";
import { Autocomplete } from "./Autocomplete";

export type Squad = { id: string; mode: "SEPARATE" | "MERGED"; ended: boolean; members: (Chip & { host: boolean })[]; host: boolean; joined: boolean; invited: boolean; pending: { username: string; expires_at: string }[] };
export type MySquads = { current: string | null; live: boolean; invites: { id: string; host: string; mode: string; expires_at: string }[] };
function UsernamePicker({ disabled, exclude }: { disabled: boolean; exclude: (string | null)[] }) {
  const [username, setUsername] = useState("");
  const [result, setResult] = useState<{ query: string; channels: Chip[]; error: string } | null>(null);
  const query = username.trim().replace(/^@/, "");
  useEffect(() => {
    if (query.length < 2 || disabled) return;
    let stopped = false;
    const timer = setTimeout(async () => {
      const response = await send<{ channels: Chip[] }>("GET", `/api/search?q=${encodeURIComponent(query)}`);
      if (!stopped) setResult({ query, channels: response.ok ? response.data.channels : [], error: response.ok ? "" : response.error });
    }, 200);
    return () => { stopped = true; clearTimeout(timer); };
  }, [query, disabled]);
  const current = result?.query === query ? result : null;
  const options = (current?.channels ?? []).filter(channel => channel.username && !exclude.some(name => name?.toLowerCase() === channel.username?.toLowerCase()))
    .slice(0, 10).map(channel => ({ value: channel.username!, label: channel.display_name === channel.username ? `@${channel.username}` : `${channel.display_name} · @${channel.username}` }));
  return <>
    <Autocomplete label="Live streamer username" name="username" required maxLength={26} disabled={disabled} value={username} onChange={setUsername} options={options} onPick={option => setUsername(option.value)} />
    <p className="small-print" role="status">{query.length < 2 ? "Type at least 2 characters to find a streamer. Both streamers must be live to invite." : !current ? "Searching…" : current.error || (!options.length ? "No other matching channels." : "Choose a streamer, then send the invitation.")}</p>
  </>;
}
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
    {initial.current ? <p><Link className="button" href={`/squads/${initial.current}`}>Open your co-stream</Link></p> : <section className="panel section"><h2>Start a co-stream</h2>{initial.live ? <form onSubmit={create}><input type="hidden" name="mode" value="MERGED" /><p>Everyone shares one chat, moderated by every member&apos;s moderators.</p><button disabled={busy}>Create co-stream</button></form> : <p><Link href="/studio/stream">Go live</Link> before creating or joining a co-stream.</p>}</section>}
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
      {squad.host && <details className="panel section" open={squad.members.length < 2}><summary>Invite streamers · {squad.members.length} of 4 streams</summary><form onSubmit={invite}><UsernamePicker disabled={busy || squad.members.length + squad.pending.length >= 4} exclude={[...squad.members.map(member => member.username), ...squad.pending.map(invite => invite.username)]} /><button disabled={busy || squad.members.length + squad.pending.length >= 4}>Send invitation</button></form>{squad.pending.map(p => <div className="row" key={p.username}><span>{p.username} · expires {new Date(p.expires_at).toLocaleTimeString()}</span><button className="quiet small" disabled={busy} onClick={() => act(`invites/${encodeURIComponent(p.username)}`, undefined, "DELETE")}>Cancel invitation</button></div>)}</details>}
      <div className="squad-layout section"><div className="squad-streams">{squad.members.map(m => m.username && <section key={m.username} className="squad-stream panel"><div className="row">{m.faction && <Crest faction={m.faction} size={24} />}<h2><Link href={`/${m.username}/live`}>{m.display_name}</Link>{m.host && " · Host"}</h2></div><LivePlayer username={m.username} signedIn={!!account} nested focused followRaids={false} muted={audio !== m.username} onUnmute={() => setAudio(m.username)} /><button className="quiet small" aria-pressed={audio === m.username} onClick={() => setAudio(audio === m.username ? null : m.username)}>{audio === m.username ? "Mute audio" : `Listen to ${m.display_name}`}</button>{account && !squad.joined && <Subscribe username={m.username} squad={squad.mode === "MERGED" ? squad.id : undefined} />}</section>)}</div>
      <aside className="squad-chat">{squad.mode === "SEPARATE" && <label className="field">Channel chat<select value={chosen ?? ""} onChange={e => setChat(e.target.value)}>{squad.members.map(m => m.username && <option key={m.username} value={m.username}>{m.display_name}</option>)}</select></label>}{squad.mode === "MERGED" ? host && <Chat key={squad.id} username={host} squad={squad.id} account={account} /> : chosen && <Chat key={chosen} username={chosen} account={account} />}</aside></div>
    </>}
  </div>;
}
/** Watch-page view of a co-stream: every stream at once or just one, audio from one stream at a time. */
export function CoStreamPlayers({ initial, focus, account }: { initial: Squad; focus: string; account: string | null }) {
  const [squad, setSquad] = useState(initial);
  const [solo, setSolo] = useState<string | null>(null);
  const [audio, setAudio] = useState<string | null>(focus);
  useEffect(() => {
    const timer = setInterval(() => { if (!document.hidden) void send<Squad>("GET", `/api/squads/${squad.id}`).then(r => { if (r.ok) setSquad(r.data); }); }, 10000);
    return () => clearInterval(timer);
  }, [squad.id]);
  const members = squad.ended ? [] : squad.members.filter(m => m.username);
  const shown = solo && members.some(m => m.username === solo) ? members.filter(m => m.username === solo) : members.length > 1 ? [...members].sort((a, b) => Number(b.username === focus) - Number(a.username === focus)) : [];
  if (!shown.length) return <div className="watch-player"><LivePlayer username={focus} focused signedIn={!!account} /></div>;
  return <div className="watch-player costream">
    <div className="costream-bar row"><span className="eyebrow">Co-stream · {members.length} streams</span>
      <button type="button" className="small quiet" aria-pressed={!solo} onClick={() => setSolo(null)}>All streams</button>
      {members.map(m => <button key={m.username} type="button" className="small quiet" aria-pressed={solo === m.username} onClick={() => { setSolo(m.username); setAudio(m.username); }}>Only {m.display_name}</button>)}
    </div>
    <div className={shown.length > 1 ? "costream-grid" : undefined}>{shown.map(m => m.username && <section key={m.username} className="squad-stream" aria-label={m.display_name}>
      <LivePlayer username={m.username} signedIn={!!account} nested focused followRaids={false} muted={audio !== m.username} onUnmute={() => setAudio(m.username)} />
      <div className="row">{m.faction && <Crest faction={m.faction} size={20} />}<Link href={`/${m.username}`}><strong>{m.display_name}</strong></Link>
        <button type="button" className="small quiet" aria-pressed={audio === m.username} onClick={() => setAudio(audio === m.username ? null : m.username)}>{audio === m.username ? "Mute" : "Listen"}</button></div>
    </section>)}</div>
  </div>;
}
