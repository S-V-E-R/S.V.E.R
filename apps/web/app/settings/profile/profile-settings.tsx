"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Avatar } from "../../../components/Avatar";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { linkUrl, platformNames, type Link as SocialLink, type Sizes } from "../../../lib/types";
import { PersonaPreview } from "../../../components/persona/PersonaPreview";
import { CosmeticsEditor } from "../../../components/persona/CosmeticsEditor";
import { HeaderCopyEditor } from "../../../components/persona/HeaderCopyEditor";
import { SongEditor } from "../../../components/persona/SongEditor";
import { LogoStylePicker, type LogoStyle } from "../../../components/LogoStyle";
import { CurrentlyEditor, HomeEditor, SongWhyEditor, ThemeEditor } from "../../../components/persona/PersonaFields";

export type Me = { username: string; display_name: string; bio: string; mood_emoji: string; status_text: string; avatar: Sizes; banner: Sizes; links: SocialLink[]; platforms: string[]; revisions: { profile: number; links: number }; rename: { next_allowed_at: string | null; held_names: { name: string; released_at: string }[] }; mfa_enabled: boolean; restricted_until: string | null; internal: boolean; mood_presets: string[]; show_linked_accounts: boolean; logo_style: LogoStyle };
type Suggestions = { suggestions: { platform: string; url: string; label: string }[]; missing_handle: string[] };
const zone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;
const date = (iso: string) => new Date(iso).toLocaleDateString(undefined, { dateStyle: "long" });

/** Your profile data, reloaded after each save, plus a version the preview follows. */
export function useMe() {
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState("");
  const [version, setVersion] = useState(0);
  const refresh = useCallback(async () => {
    const result = await send<Me>("GET", "/api/me/profile");
    if (result.ok) { setMe(result.data); setVersion(v => v + 1); } else setError(result.error);
  }, []);
  useLoad(refresh);
  const bump = useCallback(() => setVersion(v => v + 1), []);
  return { me, error, refresh, version, bump };
}

const PARTS: [string, string][] = [["look", "Look"], ["words", "Words"], ["sound", "Sound"], ["home", "Home"], ["links", "Links"], ["account", "Username"]];

/**
 * Your page (docs/PROFILES.md "Persona pass"): everything that makes your page yours, in one
 * place, with a preview of your header that follows each save.
 */
export default function ProfileSettings() {
  const { me, error, refresh, version, bump } = useMe();
  if (!me) return <p className="loading">{error || "Loading…"}</p>;
  return <div className="persona-editor">
    <div className="persona-main">
      <header className="page-head"><span className="eyebrow">Your page</span><h1>Make it yours</h1>
        <p className="lead">Your look, your words, your song and your Home. Changes show on <Link href={`/${me.username}`}>your page</Link> as soon as you save. New here? <Link href="/settings/profile/setup">Set it up step by step</Link>.</p></header>
      <nav className="persona-jump" aria-label="Sections">{PARTS.map(([id, label]) => <a key={id} href={`#${id}`}>{label}</a>)}</nav>
      {me.restricted_until && <p className="panel notice">Your channel is restricted. See <Link href="/settings/standing">Account standing</Link>.</p>}
      <div id="look" className="persona-part"><h2 className="part-title">Look</h2>
        <Images me={me} onSaved={refresh} />
        <ThemeEditor onSaved={bump} />
        <CosmeticsEditor embedded onSaved={bump} />
        <Section title="Site logo" intro="Optional. Pick the S.V.E.R badge you see in the top bar. Only you see it."><LogoStylePicker value={me.logo_style} /></Section>
      </div>
      <div id="words" className="persona-part"><h2 className="part-title">Words</h2>
        <Identity me={me} onSaved={refresh} />
        <CurrentlyEditor onSaved={bump} />
        <HeaderCopyEditor embedded onSaved={bump} />
        <p className="muted">Longer About me blocks and questions live in <Link href="/studio/channel/blocks">Creator Studio → About me</Link>.</p>
      </div>
      <div id="sound" className="persona-part"><h2 className="part-title">Sound</h2>
        <SongEditor embedded onSaved={bump} />
        <section className="panel section"><SongWhyEditor onSaved={bump} /></section>
      </div>
      <div id="home" className="persona-part"><h2 className="part-title">Home</h2>
        <HomeEditor onSaved={bump} />
        <p className="muted">Your War Council, Wall, schedule and sponsors are in <Link href="/studio/channel#war-council">Creator Studio</Link>.</p>
      </div>
      <div id="links" className="persona-part"><h2 className="part-title">Links</h2>
        <Links me={me} onSaved={refresh} />
        <Card me={me} onSaved={refresh} />
      </div>
      <div id="account" className="persona-part"><h2 className="part-title">Username</h2>
        <Username me={me} onSaved={refresh} />
        <details className="panel section claim-disclosure"><summary>Claim an inactive username</summary><Claim /></details>
      </div>
    </div>
    <PersonaPreview username={me.username} version={version} />
  </div>;
}

export function Username({ me, onSaved }: { me: Me; onSaved: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const result = await send<{ username: string }>("POST", "/api/me/username", { username: form.get("username"), code: form.get("code") || "", timezone: zone() });
    setState(result.ok ? { saved: `Your username is now @${result.data.username}.` } : result);
    if (result.ok) onSaved();
  }
  return <Section title="Username" intro="You can change your username once every 60 days. Changing only capitalization is always allowed. Your old name redirects to your channel for 30 days and nobody else can take it during that time.">
    <form onSubmit={submit}>
      <label className="field"><span>Username</span><input name="username" defaultValue={me.username} minLength={3} maxLength={25} pattern="[A-Za-z0-9_]{3,25}" required /></label>
      {me.mfa_enabled && <label className="field"><span>Authenticator code</span><input name="code" inputMode="numeric" autoComplete="one-time-code" maxLength={8} /></label>}
      {me.rename.next_allowed_at && <p className="muted">You can change your username again on {date(me.rename.next_allowed_at)}.</p>}
      {me.rename.held_names.map(h => <p key={h.name} className="muted">@{h.name} is held for you until {date(h.released_at)}. You can switch back to it once.</p>)}
      <p className="muted small-print">For your security, you may need to <Link href="/account">confirm your sign-in</Link> first.</p>
      <button type="submit" className="small">Change username</button>
      <Status state={state} />
    </form>
  </Section>;
}

type ClaimRow = { id: string; username: string; status: "WAITING" | "KEPT" | "READY" | "RELEASED" | "REJECTED" | "WITHDRAWN"; created_at: string; reason: string | null };
const claimStatus: Record<ClaimRow["status"], string> = { WAITING: "Waiting for the holder (30 days)", KEPT: "The holder kept it", READY: "Waiting for staff", RELEASED: "Yours", REJECTED: "Not released", WITHDRAWN: "Withdrawn" };

/** Username claims: ask for a name held by an account inactive for 12 months. */
export function Claim() {
  const [claims, setClaims] = useState<ClaimRow[]>([]);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<{ claims: ClaimRow[] }>("GET", "/api/me/username-claims");
    if (r.ok) setClaims(r.data.claims);
  }, []);
  useLoad(load);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const r = await send("POST", "/api/me/username-claims", { username: new FormData(event.currentTarget).get("username") });
    setState(r.ok ? { saved: "Asked. The holder has 30 days to sign in and keep it." } : r);
    await load();
  }
  async function withdraw(id: string) {
    const r = await send<{ claims: ClaimRow[] }>("DELETE", `/api/me/username-claims/${encodeURIComponent(id)}`);
    if (r.ok) setClaims(r.data.claims);
  }
  return <Section title="Claim an inactive username" intro="If the name you want belongs to an account nobody has used for 12 months, you can ask for it. The holder is emailed and keeps it by signing in within 30 days; otherwise staff review it. One open claim at a time, never for money.">
    <form onSubmit={submit}>
      <label className="field"><span>Username</span><input name="username" minLength={3} maxLength={25} pattern="[A-Za-z0-9_]{3,25}" required /></label>
      <button type="submit" className="small">Ask for this name</button>
      <Status state={state} />
    </form>
    {claims.length > 0 && <ul className="list">{claims.map(c => <li key={c.id} className="row">
      <strong>@{c.username}</strong> <span className="muted">{claimStatus[c.status]} · {date(c.created_at)}</span>
      {(c.status === "WAITING" || c.status === "READY") && <button type="button" className="small quiet" onClick={() => withdraw(c.id)}>Withdraw</button>}
    </li>)}</ul>}
  </Section>;
}

export function Identity({ me, onSaved }: { me: Me; onSaved: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [mood, setMood] = useState(me.mood_emoji);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const result = await send("PATCH", "/api/me/profile", { display_name: form.get("display_name"), bio: form.get("bio"), mood_emoji: mood, status_text: form.get("status_text"), revision: me.revisions.profile });
    setState(result.ok ? { saved: "Saved." } : result);
    if (result.ok) onSaved();
  }
  const err = (f: string) => state.field === f ? state.error : undefined;
  return <Section title="Identity">
    <form onSubmit={submit} noValidate>
      <label className="field"><span>Display name</span><input name="display_name" defaultValue={me.display_name} maxLength={32} aria-invalid={!!err("display_name")} /><small>Up to 32 characters. Your @{me.username} always shows next to it.</small></label>
      <label className="field"><span>Bio</span><textarea name="bio" defaultValue={me.bio} maxLength={300} rows={4} aria-invalid={!!err("bio")} /><small>Up to 300 characters.</small></label>
      <div className="row"><label className="field narrow"><span>Mood</span><input name="mood_emoji" value={mood} onChange={e => setMood(e.target.value)} maxLength={16} aria-invalid={!!err("mood_emoji")} /><small>One emoji.</small></label>
        <label className="field grow"><span>Status</span><input name="status_text" defaultValue={me.status_text} maxLength={80} aria-invalid={!!err("status_text")} /><small>Up to 80 characters.</small></label></div>
      <div className="mood-presets" role="group" aria-label="Mood presets">{me.mood_presets.map(p => <button key={p} type="button" className="small quiet emoji" aria-pressed={mood === p} aria-label={`Mood ${p}`} onClick={() => setMood(p)}>{p}</button>)}<button type="button" className="small quiet" onClick={() => setMood("")}>Clear</button></div>
      <button type="submit" className="small">Save identity</button>
      <Status state={state} />
    </form>
  </Section>;
}

export function Images({ me, onSaved }: { me: Me; onSaved: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function upload(kind: "avatar" | "banner", file: File | undefined) {
    if (!file) return;
    const form = new FormData();
    form.append("file", file);
    setState({ saved: "Uploading…" });
    const result = await send("POST", `/api/me/${kind}`, form);
    setState(result.ok ? { saved: kind === "avatar" ? "Avatar updated." : "Banner updated." } : result);
    if (result.ok) onSaved();
  }
  async function remove(kind: "avatar" | "banner") {
    const result = await send("DELETE", `/api/me/${kind}`);
    setState(result.ok ? { saved: "Removed." } : result);
    if (result.ok) onSaved();
  }
  const banner = me.banner ? Object.values(me.banner)[0] : null;
  return <Section title="Avatar and banner" intro="JPG, PNG or WebP. Avatars are cropped square (at least 128×128, up to 5 MB); banners are cropped to 3:1 (at least 1200×400, up to 10 MB). Image metadata is removed.">
    <div className="row"><Avatar sizes={me.avatar} name={me.display_name} size={96} />
      <label className="button small quiet">Upload avatar<input className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" onChange={e => upload("avatar", e.target.files?.[0])} /></label>
      {me.avatar && <button type="button" className="small quiet" onClick={() => remove("avatar")}>Remove avatar</button>}</div>
    {/* eslint-disable-next-line @next/next/no-img-element */}
    {banner ? <img className="banner-preview" src={banner} alt="Current banner" /> : <div className="banner-preview default-banner" aria-hidden="true" />}
    <div className="row"><label className="button small quiet">Upload banner<input className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" onChange={e => upload("banner", e.target.files?.[0])} /></label>
      {me.banner && <button type="button" className="small quiet" onClick={() => remove("banner")}>Remove banner</button>}</div>
    <Status state={state} />
  </Section>;
}

export function Links({ me, onSaved }: { me: Me; onSaved: () => void }) {
  const [links, setLinks] = useState<SocialLink[]>(me.links);
  const [state, setState] = useState<SaveState>({});
  const [hints, setHints] = useState<Suggestions | null>(null);
  const loadHints = useCallback(async () => { const r = await send<Suggestions>("GET", "/api/me/link-suggestions"); if (r.ok) setHints(r.data); }, []);
  useLoad(loadHints);
  // Suggestions only fill the form (decision P5); nothing is saved until Save links.
  const pending = (hints?.suggestions ?? []).filter(s => !links.some(l => l.platform === s.platform));
  async function save(event: FormEvent) {
    event.preventDefault();
    const result = await send<{ links: SocialLink[] }>("PUT", "/api/me/links", { links: links.map(l => ({ ...l, url: linkUrl(l.platform, l.url) })).filter(l => l.url), revision: me.revisions.links });
    setState(result.ok ? { saved: "Links saved." } : result);
    if (result.ok) { setLinks(result.data.links); onSaved(); loadHints(); }
  }
  const update = (i: number, patch: Partial<SocialLink>) => setLinks(links.map((l, j) => j === i ? { ...l, ...patch } : l));
  return <Section title="Social links" intro="Up to 5 links. Type your handle (we build the link) or paste a full https:// address on the platform's own site; Website can be used twice.">
    <form onSubmit={save}>
      {links.map((l, i) => <div className="row" key={i}>
        <label className="field narrow"><span className="sr-only">Platform</span><select value={l.platform} onChange={e => update(i, { platform: e.target.value })}>{me.platforms.map(p => <option key={p} value={p}>{platformNames[p] || p}</option>)}</select></label>
        <label className="field grow"><span className="sr-only">Handle or link</span><input type="text" inputMode="url" autoCapitalize="none" spellCheck={false} maxLength={2048} value={l.url} placeholder="Handle or https://" onChange={e => update(i, { url: e.target.value })} /></label>
        <button type="button" className="small quiet" onClick={() => setLinks(links.filter((_, j) => j !== i))}>Remove</button>
      </div>)}
      <div className="row">{links.length < 5 && <button type="button" className="small quiet" onClick={() => setLinks([...links, { platform: "website", url: "" }])}>Add link</button>}<button type="submit" className="small">Save links</button></div>
      {(pending.length > 0 || hints?.missing_handle.includes("twitch")) && <div className="suggestions panel" aria-label="From your linked accounts">
        <p className="eyebrow">FROM YOUR LINKED ACCOUNTS</p>
        {pending.map(s => <div key={s.platform} className="row between"><span>{platformNames[s.platform] || s.platform} <small className="muted">{s.url}</small></span><button type="button" className="small quiet" disabled={links.length >= 5} title={links.length >= 5 ? "You already have 5 links" : undefined} onClick={() => setLinks([...links, { platform: s.platform, url: s.url }])}>Add</button></div>)}
        {links.length >= 5 && pending.length > 0 && <p className="muted">You already have 5 links.</p>}
        {hints?.missing_handle.includes("twitch") && !links.some(l => l.platform === "twitch") && <p className="muted">Sign in with Twitch once to suggest your channel link.</p>}
        <p className="muted small-print">Added links are saved only when you choose Save links.</p>
      </div>}
      <Status state={state} />
    </form>
  </Section>;
}

type DiscordRank = { linked: boolean; shown: boolean; rank: { name: string; level: number } | null };

export function Card({ me, onSaved }: { me: Me; onSaved: () => void }) {
  const [state, setState] = useState<SaveState>({});
  const [discord, setDiscord] = useState<DiscordRank | null>(null);
  const loadDiscord = useCallback(async () => {
    const r = await send<DiscordRank>("GET", "/api/me/discord-rank");
    if (r.ok) setDiscord(r.data);
  }, []);
  useLoad(loadDiscord);
  async function showRank(value: boolean) {
    const result = await send<DiscordRank>("PUT", "/api/me/discord-rank", { shown: value });
    if (result.ok) setDiscord(result.data);
    setState(result.ok ? { saved: value ? "Your Discord rank now shows on your page." : "Your Discord rank is hidden." } : result);
  }
  async function toggle(value: boolean) {
    const result = await send("PUT", "/api/me/card-settings", { show_linked_accounts: value });
    setState(result.ok ? { saved: value ? "Your linked accounts now show on your user card." : "Your linked accounts are hidden." } : result);
    if (result.ok) onSaved();
  }
  return <Section title="User card" intro="Your user card appears when people tap your name in lists, the Wall and chat.">
    <label className="checkbox"><input type="checkbox" checked={me.show_linked_accounts} onChange={e => toggle(e.target.checked)} /> Show my linked Twitch and Discord accounts on my user card</label>
    <p className="muted small-print">Off unless you turn it on. Shows as &ldquo;Also known as&rdquo;; Discord names show as text only. <Link href="/account">Manage linked accounts</Link>.</p>
    {discord && <label className="checkbox"><input type="checkbox" checked={discord.shown} disabled={!discord.linked} onChange={e => showRank(e.target.checked)} /> Show my rank on the S.V.E.R Discord server on my page{discord.rank ? ` (${discord.rank.name}, level ${discord.rank.level})` : ""}</label>}
    {discord && !discord.linked && <p className="muted small-print">Link Discord on your <Link href="/account">account page</Link> to show your Discord rank.</p>}
    <Status state={state} />
  </Section>;
}
