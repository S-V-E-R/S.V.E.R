"use client";
import Link from "next/link";
import { useCallback, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { send, useLoad } from "../lib/client-api";
import type { Guild, GuildPage, MyGuilds } from "../lib/guilds";
import type { Chip } from "../lib/types";
import { Avatar } from "./Avatar";
import { Crest } from "./FactionIdentity";
import { ReportButton, TakeDownLink } from "./Report";
import "../styles/teams.css";

export function GuildChatBadge({ guild }: { guild: NonNullable<Chip["guild"]> }) {
  return <Link className="guild-badge" href={`/g/${guild.slug}`} title={guild.name} aria-label={`${guild.tag} — ${guild.name}`}>
    {/* The media service has already resized this original emblem. */}
    {/* eslint-disable-next-line @next/next/no-img-element */}
    {guild.image ? <img src={guild.image} alt={guild.tag} width={18} height={18} onError={e => { e.currentTarget.hidden = true; const fallback = e.currentTarget.nextElementSibling; if (fallback instanceof HTMLElement) fallback.hidden = false; }} /> : null}<span hidden={!!guild.image}>{guild.tag}</span>
  </Link>;
}

export function GuildEmblem({ guild, size = 56 }: { guild: Pick<Guild, "name" | "tag" | "avatar">; size?: number }) {
  const [failed, setFailed] = useState(false);
  return guild.avatar && !failed
    // Original raster art, served at fixed processed sizes by the media service.
    // eslint-disable-next-line @next/next/no-img-element
    ? <img className="guild-emblem" src={guild.avatar} width={size} height={size} alt={`${guild.tag} — ${guild.name}`} onError={() => setFailed(true)} />
    : <span className="guild-emblem guild-tag" style={{ width: size, minHeight: size }} role="img" aria-label={`${guild.tag} — ${guild.name}`}>{guild.tag}</span>;
}
export function GuildCard({ guild }: { guild: Guild }) {
  return <article className="panel guild-card"><GuildEmblem guild={guild} /><div><h2><Link href={`/g/${guild.slug}`}>{guild.name}</Link></h2><p>{guild.tagline || "A cross-faction stream team."}</p><span className="muted">{guild.status === "ARCHIVED" ? "Archived" : guild.recruiting ? "Recruiting" : "Not recruiting"}{guild.verified && " · Verified organization"}{guild.role && ` · ${guild.role}`}</span></div></article>;
}
function IdentityFields({ guild }: { guild?: Guild }) {
  return <>
    <label className="field">Name<input name="name" required minLength={3} maxLength={40} defaultValue={guild?.name} /></label>
    <label className="field">URL name<input name="slug" required pattern="[A-Za-z0-9_]{3,25}" maxLength={25} readOnly={!!guild} defaultValue={guild?.slug} /><span className="muted small">sver.tv/g/your_name · Letters, numbers and underscores.</span></label>
    <label className="field">Tag<input name="tag" required minLength={2} maxLength={5} pattern="[A-Za-z0-9]{2,5}" defaultValue={guild?.tag} /></label>
    <label className="field">Tagline<input name="tagline" maxLength={120} defaultValue={guild?.tagline} /></label>
    <label className="field">About<textarea name="about" rows={5} maxLength={3000} defaultValue={guild?.about} /></label>
    <label className="checkbox"><input name="recruiting" type="checkbox" defaultChecked={guild?.recruiting ?? true} /> Accept applications</label>
  </>;
}
export function GuildStudio({ initial, account }: { initial: MyGuilds; account: string }) {
  const router = useRouter();
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [mates, setMates] = useState<Chip[]>([]);
  const loadMates = useCallback(async () => {
    const pages = await Promise.all(initial.items.filter(g => g.role && g.status === "ACTIVE").map(g => send<GuildPage>("GET", `/api/guilds/${g.slug}`)));
    const members = new Map<string, Chip>();
    for (const p of pages) if (p.ok) for (const m of p.data.members) if (m.live && m.user.username && m.user.username !== account) members.set(m.user.username, m.user);
    setMates([...members.values()]);
  }, [initial.items, account]);
  useLoad(loadMates);
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true);
    const data = new FormData(event.currentTarget);
    const result = await send<{ slug: string }>("POST", "/api/guilds", { ...Object.fromEntries(data), recruiting: data.has("recruiting") });
    if (result.ok) router.push(`/g/${result.data.slug}/settings`); else setNotice(result.error);
    setBusy(false);
  }
  return <><h1>Guilds</h1><p>Stream teams across all three factions. Join up to three guilds and choose one emblem for chat.</p><p><Link href="/guilds">Find a guild</Link> · <Link href="/studio/squads">Co-streams</Link></p>
    <div className="guild-grid">{initial.items.map(g => <div key={g.id}><GuildCard guild={g} />{g.role && <Link className="button quiet" href={`/g/${g.slug}`}>Members &amp; team shortcuts</Link>}</div>)}</div>
    {initial.items.length === 0 && <p className="muted">Your memberships, applications and invitations will appear here.</p>}
    <section className="section"><h2>Live guildmates</h2>{mates.length === 0 ? <p className="muted">No guildmates are live right now.</p> : <div className="guild-grid">{mates.map(m => <article className="panel guild-member" key={m.username}><Person user={m} /><TeamShortcuts username={m.username!} report={setNotice} /></article>)}</div>}</section>
    <section className="panel section"><h2>Chat emblem</h2><label className="field">Show a guild beside your name<select defaultValue={initial.badge ?? ""} onChange={async e => { const r = await send("PUT", "/api/me/guilds/badge", { guild_id: e.target.value || null }); setNotice(r.ok ? "Chat emblem saved." : r.error); }}><option value="">Hide guild emblem</option>{initial.items.filter(g => g.role && g.status === "ACTIVE").map(g => <option key={g.id} value={g.id}>{g.name} [{g.tag}]</option>)}</select></label></section>
    <section className="panel section"><h2>Create a guild</h2>{initial.can_create ? <form onSubmit={create}><IdentityFields /><button disabled={busy}>Create guild</button></form> : <p>Verify your email, enable authenticator 2FA and complete your first stream to create a guild. You can lead one guild.</p>}</section>
    {notice && <p role="status" className="form-message">{notice}</p>}
  </>;
}
export function TeamShortcuts({ username, report }: { username: string; report: (message: string) => void }) {
  const [busy, setBusy] = useState(false);
  async function run(action: "raid" | "host" | "auto" | "squad") {
    setBusy(true);
    if (action === "squad") {
      const mine = await send<{ current: string | null }>("GET", "/api/me/squads");
      if (!mine.ok) report(mine.error);
      else if (!mine.data.current) report("Create a co-stream in Creator Studio, then invite this guildmate.");
      else { const r = await send("POST", `/api/squads/${mine.data.current}/invites`, { username }); report(r.ok ? `Invitation sent to ${username}.` : r.error); }
    } else if (action === "auto") {
      const r = await send<{ accept_raids: boolean; accept_hosts: boolean; auto_host: boolean; auto_list: string[] }>("GET", "/api/me/hosting");
      if (!r.ok) report(r.error);
      else { const saved = await send("PUT", "/api/me/hosting", { ...r.data, auto_host: true, auto_list: [...new Set([...r.data.auto_list, username])] }); report(saved.ok ? `${username} added to auto-host.` : saved.error); }
    } else {
      const r = await send(action === "raid" ? "POST" : "PUT", action === "raid" ? "/api/me/raids" : "/api/me/hosting/target", { username }); report(r.ok ? action === "raid" ? "Raid started." : `Hosting ${username}.` : r.error);
    }
    setBusy(false);
  }
  return <div className="row"><button className="small quiet" disabled={busy} onClick={() => run("raid")}>Raid</button><button className="small quiet" disabled={busy} onClick={() => run("host")}>Host</button><button className="small quiet" disabled={busy} onClick={() => run("squad")}>Co-stream invite</button><button className="small quiet" disabled={busy} onClick={() => run("auto")}>Auto-host</button></div>;
}
function Person({ user }: { user: Chip }) { return <span className="row"><Avatar sizes={user.avatar} name={user.display_name} size={32} />{user.faction && <Crest faction={user.faction} size={24} />}{user.linked && user.username ? <Link href={`/${user.username}`}>{user.display_name}</Link> : <span>{user.display_name}</span>}</span>; }

export function GuildView({ initial, settings = false, account }: { initial: GuildPage; settings?: boolean; account: string | null }) {
  const [page, setPage] = useState(initial);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const g = page.guild, v = page.viewer;
  const path = `/api/guilds/${g.slug}`;
  const reload = useCallback(async () => { const r = await send<GuildPage>("GET", path); if (r.ok) setPage(r.data); else setNotice(r.error); }, [path]);
  async function act(body: Record<string, unknown>) {
    setBusy(true); const r = await send("POST", `${path}/actions`, body); setNotice(r.ok ? "Saved." : r.error); if (r.ok) await reload(); setBusy(false);
  }
  async function actionForm(event: FormEvent<HTMLFormElement>, action: string, username?: string) {
    event.preventDefault(); await act({ ...Object.fromEntries(new FormData(event.currentTarget)), action, ...(username ? { username } : {}) });
  }
  async function branding(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); const data = new FormData(event.currentTarget);
    const r = await send("PATCH", path, { ...Object.fromEntries(data), recruiting: data.has("recruiting"), revision: g.revision }); setNotice(r.ok ? "Guild updated." : r.error); if (r.ok) await reload(); setBusy(false);
  }
  async function upload(event: FormEvent<HTMLFormElement>, kind: string) {
    event.preventDefault(); setBusy(true); const r = await send("POST", `${path}/media/${kind}`, new FormData(event.currentTarget)); setNotice(r.ok ? "Image uploaded." : r.error); if (r.ok) await reload(); setBusy(false);
  }
  const manager = page.management;
  const banner = g.banner ? Object.values(g.banner).at(-1) : null;
  return <div className="guild-page">
    <header className="frame guild-header">
      {/* The media service already resizes banners. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {banner && <img className="guild-banner" src={banner} alt="" />}
      <div className="guild-identity"><GuildEmblem key={g.avatar} guild={g} size={96} /><div><span className="eyebrow">Cross-faction stream team</span><h1>{g.name}</h1><p>{g.tagline}</p><span className="muted">{g.status === "ACTIVE" ? g.recruiting ? "Recruiting" : "Not recruiting" : "Archived"}{g.verified && " · Verified organization"}</span></div></div>
    </header>
    <nav className="row guild-nav" aria-label="Guild"><Link href={`/g/${g.slug}`} aria-current={!settings ? "page" : undefined}>Guild page</Link><Link href="/guilds">All guilds</Link>{v.can_manage && <Link href={`/g/${g.slug}/settings`} aria-current={settings ? "page" : undefined}>Manage guild</Link>}{v.signed_in && <Link href="/studio/guilds">My guilds</Link>}</nav>
    {notice && <p role="status" className="form-message">{notice}</p>}
    {!settings && <>
      <div className="row">{v.signed_in ? <><button disabled={busy || (g.status !== "ACTIVE" && !v.following)} className="small" onClick={() => act({ action: v.following ? "unfollow" : "follow" })}>{v.following ? "Unfollow guild" : "Follow guild"}</button><button className="small quiet" disabled={busy} onClick={() => act({ action: v.muted ? "unmute" : "mute" })}>{v.muted ? "Unmute guild" : "Mute guild"}</button>{v.role && <><button className="small quiet" disabled={busy} onClick={async () => { const r = await send("PUT", "/api/me/guilds/badge", { guild_id: v.badge ? null : g.id }); setNotice(r.ok ? "Chat emblem saved." : r.error); if (r.ok) await reload(); }}>{v.badge ? "Hide chat emblem" : "Use chat emblem"}</button><button className="small quiet" disabled={busy} onClick={() => { if (window.confirm(v.role === "leader" ? "Leave this guild? Leadership passes to an eligible officer, or the guild is archived." : "Leave this guild? Rejoining requires a new application.")) void act({ action: "leave" }); }}>Leave guild</button></>}</> : <Link className="button" href="/login">Sign in to follow or apply</Link>}</div>
      {g.about && <section className="panel section"><h2>About the guild</h2><p className="guild-copy">{g.about}</p></section>}
      {v.signed_in && !v.role && g.status === "ACTIVE" && <section className="panel section"><h2>Apply to join</h2>{v.invited && <p>You&apos;ve been invited to apply. Send a message to the team.</p>}{v.application && <p>Application: {v.application.status.toLowerCase()}. {v.application.note}{v.application.status === "DECLINED" && v.application.reapply_at && <> You can reapply from {new Date(v.application.reapply_at).toLocaleDateString()}.</>}</p>}{v.application?.status === "OPEN" ? <button disabled={busy} onClick={() => act({ action: "withdraw" })}>Withdraw application</button> : g.recruiting ? <form onSubmit={e => actionForm(e, "apply")}><label className="field">Tell the team about your streams<textarea name="message" required maxLength={500} rows={3} /></label><button disabled={busy}>Send application</button></form> : <p>This guild isn&apos;t accepting applications right now.</p>}</section>}
      <section className="section"><h2>Members</h2>{v.role && <p className="muted">Team shortcuts use your channel. <Link href="/studio/squads">Create or manage a co-stream</Link>.</p>}<div className="guild-grid">{page.members.map((m, i) => <article className="panel guild-member" key={m.user.username ?? i}><Person user={m.user} /><p>{m.role}{m.title && ` · ${m.title}`}{m.live && <span className="badge live">LIVE</span>}</p>{m.live && m.user.username && <Link className="button small" href={`/${m.user.username}/live`}>Watch</Link>}{v.role && m.live && m.user.username && m.user.username !== account && <TeamShortcuts username={m.user.username} report={setNotice} />}</article>)}</div>{page.members.length === 0 && <p className="muted">No public members to display.</p>}</section>
      <section className="panel section"><h2>Team schedule</h2><p className="muted">Upcoming streams in your local time.</p>{page.schedule.length === 0 ? <p>No streams scheduled in the next two weeks.</p> : <ul className="list">{page.schedule.map(({ user, occurrence: o }, i) => <li key={i}><Person user={user} /><strong>{o.label}</strong><p><time dateTime={o.start_at}>{new Date(o.start_at).toLocaleString()}</time> – {new Date(o.end_at).toLocaleTimeString()}</p></li>)}</ul>}</section>
      <div className="row">{v.signed_in && <><ReportButton target={{ target_type: "guild", target_id: g.id }} label="Report guild" />{g.avatar && <ReportButton target={{ target_type: "guild_emblem", target_id: g.id }} label="Report emblem" />}</>}<TakeDownLink target={{ target_type: "guild", target_id: g.id }} /></div>
    </>}
    {settings && (!manager ? <p>You don&apos;t manage this guild.</p> : <>
      <section className="panel section"><h2>Applications</h2>{manager.applications.length === 0 && <p>No pending applications.</p>}{manager.applications.map(a => <article className="guild-application" key={a.id}><Person user={a.user} /><p className="guild-copy">{a.message}</p><form onSubmit={e => { const action = (e.nativeEvent as SubmitEvent).submitter?.getAttribute("value") ?? "decline"; void actionForm(e, action, a.user.username ?? ""); }}><label className="field">Optional decision note<textarea name="note" maxLength={500} rows={2} /></label><div className="row"><button value="accept" disabled={busy}>Accept</button><button className="quiet" value="decline" disabled={busy}>Decline</button></div></form></article>)}</section>
      <section className="panel section"><h2>Invite to apply</h2><form onSubmit={e => actionForm(e, "invite")}><label className="field">Streamer username<input name="username" required maxLength={25} /></label><button disabled={busy}>Send invitation</button></form></section>
      <section className="panel section"><h2>Members &amp; roles</h2>{page.members.map((m, i) => <article className="guild-application" key={m.user.username ?? i}><Person user={m.user} /><p>{m.role}{m.title && ` · ${m.title}`}</p>{m.user.username && m.user.username !== account && <><form onSubmit={e => actionForm(e, "title", m.user.username!)}><label className="field">Member title<input name="message" maxLength={40} defaultValue={m.title} /></label><button className="small quiet" disabled={busy}>Save title</button></form><div className="row">{v.role === "leader" && <><button className="small quiet" disabled={busy} onClick={() => act({ action: "officer", username: m.user.username, enabled: m.role !== "officer" })}>{m.role === "officer" ? "Remove officer role" : "Make officer"}</button><button className="small quiet" disabled={busy} onClick={() => { if (window.confirm(`Transfer leadership to ${m.user.username}?`)) void act({ action: "transfer", username: m.user.username }); }}>Transfer leadership</button></>}{(v.role === "leader" || m.role === "member") && <button className="small quiet" disabled={busy} onClick={() => { if (window.confirm(`Remove ${m.user.username} from the guild?`)) void act({ action: "remove", username: m.user.username }); }}>Remove member</button>}</div></>}</article>)}</section>
      <section className="panel section"><h2>Blocked streamers</h2><form onSubmit={e => actionForm(e, "block")}><label className="field">Username<input name="username" required maxLength={25} /></label><button disabled={busy}>Block applications &amp; remove membership</button></form><ul className="list">{manager.blocks.map(name => <li className="row" key={name}>{name}<button className="small quiet" disabled={busy} onClick={() => act({ action: "unblock", username: name })}>Unblock</button></li>)}</ul></section>
      {v.role === "leader" && <>
        <section className="panel section"><h2>Guild identity</h2><form key={g.revision} onSubmit={branding}><IdentityFields guild={g} /><button disabled={busy}>Save guild</button></form></section>
        <section className="panel section"><h2>Images</h2><p>Use original artwork. Emblems cannot imitate site, faction or verified marks.</p>{["avatar", "banner"].map(kind => <form key={kind} onSubmit={e => upload(e, kind)}><label className="field">{kind === "avatar" ? "Emblem: square PNG or WebP, at least 112 px" : "Banner: PNG, WebP or JPEG"}<input name="file" type="file" required accept={kind === "avatar" ? "image/png,image/webp" : "image/png,image/webp,image/jpeg"} /></label><div className="row"><button disabled={busy}>Upload {kind === "avatar" ? "emblem" : kind}</button><button type="button" className="quiet" disabled={busy} onClick={() => act({ action: `clear_${kind}` })}>Remove {kind === "avatar" ? "emblem" : kind}</button></div></form>)}</section>
        <section className="panel section"><h2>Organization verification</h2>{manager.verification && <p>{manager.verification.status.toLowerCase()}: {manager.verification.note}</p>}<form onSubmit={e => actionForm(e, "verification")}><label className="field">Organization and evidence of representation<textarea name="message" rows={4} maxLength={2000} required /></label><button disabled={busy || manager.verification?.status === "OPEN"}>Request review</button></form></section>
      </>}
      <section className="panel section"><h2>Membership log</h2><ul className="list">{manager.events.map(e => <li key={e.id}><time dateTime={e.created_at}>{new Date(e.created_at).toLocaleString()}</time> · {e.actor ?? "System"}: {e.action.replaceAll("_", " ")}{e.subject && ` · ${e.subject}`}</li>)}</ul></section>
    </>)}
  </div>;
}
