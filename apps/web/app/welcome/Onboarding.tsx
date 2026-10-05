"use client";
import Image from "next/image";
import Link from "next/link";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { SignupSteps } from "../screens";
import { Avatar } from "../../components/Avatar";
import { Crest } from "../../components/Crest";
import { send, useLoad } from "../../lib/client-api";
import { FACTIONS, crestSrc, factionOf, type FactionSlug } from "../../lib/factions";
import type { Chip, Sizes } from "../../lib/types";

type Step = "side" | "joined" | "profile" | "follow" | "ready";
type Standing = { faction: FactionSlug | null; can_choose: boolean; free_switch_until: string | null };
type Profile = { display_name: string; bio: string; avatar: Sizes; revisions: { profile: number } };

/**
 * The onboarding wizard (legacy /onboarding, rebuilt on S.V.E.R's rules): choose your side with
 * each faction's story, the welcome to your faction, profile, follow creators, ready. The side
 * can't be skipped (every account has a faction, docs/FACTIONS.md); profile and follows can.
 */
export function Onboarding({ username, current, pick }: { username: string; current: FactionSlug | null; pick: string | null }) {
  const [step, setStep] = useState<Step>("side");
  const [faction, setFaction] = useState<FactionSlug | null>(current);
  const [followed, setFollowed] = useState(0);

  const go = useCallback((next: Step) => { setStep(next); window.scrollTo({ top: 0 }); }, []);
  const number = ({ side: 2, joined: 2, profile: 3, follow: 4, ready: 5 } as const)[step];

  return <div className="onboarding">
    <SignupSteps current={number} />
    {step === "side" && <ChooseSide current={current} pick={factionOf(pick)?.slug ?? null} onJoined={slug => { setFaction(slug); document.documentElement.dataset.theme = slug; go("joined"); }} onKeep={() => go("profile")} />}
    {step === "joined" && faction && <Joined slug={faction} onNext={() => go("profile")} />}
    {step === "profile" && <ProfileStep username={username} onNext={() => go("follow")} onBack={() => go("joined")} />}
    {step === "follow" && <FollowStep onNext={count => { setFollowed(count); go("ready"); }} onBack={() => go("profile")} />}
    {step === "ready" && <Ready slug={faction} followed={followed} username={username} />}
  </div>;
}

function ChooseSide({ current, pick, onJoined, onKeep }: { current: FactionSlug | null; pick: FactionSlug | null; onJoined: (slug: FactionSlug) => void; onKeep: () => void }) {
  const [standing, setStanding] = useState<Standing | null>(null);
  const [choice, setChoice] = useState<FactionSlug | null>(pick ?? current);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => { send<Standing>("GET", "/api/me/faction").then(r => r.ok && setStanding(r.data)); }, []);
  const chosen = factionOf(choice);
  const locked = !!standing && !standing.can_choose;

  async function enlist() {
    if (!choice) return;
    if (choice === standing?.faction) { onKeep(); return; }
    setBusy(true); setError("");
    const result = await send<Standing>("PUT", "/api/me/faction", { faction: choice });
    setBusy(false);
    if (!result.ok) { setError(result.error); return; }
    onJoined(choice);
  }

  return <section className="onboarding-step wide" aria-labelledby="side-title">
    <header className="onboarding-head">
      <h1 id="side-title">Choose your side</h1>
      <p>Every kind of creator belongs here. Your faction isn&apos;t about what you stream; it&apos;s about how you show up. Pick the people whose beliefs feel like yours.</p>
    </header>
    {locked && current && <p className="notice">You&apos;re in {factionOf(current)?.name}. Your free switch has been used or has ended; you can switch between seasons.</p>}
    {error && <p className="notice error" role="alert">{error}</p>}
    <div className="faction-picks" role="radiogroup" aria-label="Faction">
      {FACTIONS.map(f => <button key={f.slug} type="button" role="radio" aria-checked={choice === f.slug} className={`faction-pick ${f.slug}`} disabled={locked && f.slug !== current} onClick={() => setChoice(f.slug)}>
        <Image src={crestSrc(f.slug)} width={120} height={120} alt="" unoptimized />
        <span className="faction-pick-name">{f.name}</span>
        <span className="eyebrow">{f.title}</span>
        <span className="faction-pick-creed">{f.creed}</span>
        <span className="small-text">{f.belief}</span>
        {current === f.slug && <span className="badge verified">Your side</span>}
      </button>)}
    </div>
    {chosen && <article className={`faction-story panel ${chosen.slug}`} aria-label={`About ${chosen.name}`}>
      <p className="faction-story-lore">{chosen.lore}</p>
      <dl>
        <div><dt>Who belongs here</dt><dd>{chosen.people}</dd></div>
        <div><dt>What we reject</dt><dd>{chosen.rejects}</dd></div>
        <div><dt>Home turf</dt><dd>{chosen.turf.join(" · ")}</dd></div>
      </dl>
      <ul className="trait-chips">{chosen.values.map(v => <li key={v}>{v}</li>)}</ul>
    </article>}
    <p className="small-text center">No faction is better than another. You get one free switch in your first 7 days, then only between seasons.</p>
    <div className="onboarding-actions">
      <button type="button" onClick={enlist} disabled={!choice || busy || (locked && choice !== current)} data-faction={choice ?? undefined}>{busy ? "Please wait…" : !chosen ? "Pick a faction" : choice === current ? `Continue with ${chosen.name}` : `Enlist in ${chosen.name}`}</button>
    </div>
  </section>;
}

function Joined({ slug, onNext }: { slug: FactionSlug; onNext: () => void }) {
  const f = factionOf(slug)!;
  const [until, setUntil] = useState<string | null>(null);
  useEffect(() => { send<Standing>("GET", "/api/me/faction").then(r => r.ok && setUntil(r.data.free_switch_until)); }, []);
  return <section className="onboarding-step joined frame" aria-labelledby="joined-title">
    <Image src={crestSrc(slug)} width={120} height={120} alt={`${f.name} crest`} unoptimized />
    <h1 id="joined-title">Welcome to {f.name}</h1>
    <p className="eyebrow">{f.title} · {f.creed}</p>
    <p className="joined-lore">{f.lore}</p>
    <p className="joined-call">You chose how you want to show up. Now make it real.</p>
    <div className="joined-grid">
      <div><strong>Your colors</strong><span>The whole site now wears {f.name}&apos;s colors.</span></div>
      <div><strong>Your crest</strong><span>Shown on your channel, your player card and next to your name.</span></div>
      <div><strong>Your side</strong><span><Link href="/factions">The factions page</Link> has your home turf. The war, map and hub arrive with Season 1.</span></div>
      <div><strong>Your switch</strong><span>{until ? `One free switch until ${new Date(until).toLocaleDateString(undefined, { month: "long", day: "numeric" })}, then only between seasons.` : "Switching opens between seasons."}</span></div>
    </div>
    <button type="button" className="primary" onClick={onNext}>Continue</button>
  </section>;
}

function ProfileStep({ username, onNext, onBack }: { username: string; onNext: () => void; onBack: () => void }) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [error, setError] = useState<{ message: string; field?: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [bio, setBio] = useState("");
  const load = useCallback(async () => {
    const r = await send<Profile>("GET", "/api/me/profile");
    if (r.ok) { setProfile(r.data); setBio(r.data.bio); }
  }, []);
  useLoad(load);

  async function upload(file: File | undefined) {
    if (!file) return;
    const form = new FormData();
    form.append("file", file);
    setBusy(true);
    const r = await send("POST", "/api/me/avatar", form);
    setBusy(false);
    if (!r.ok) setError({ message: r.error }); else { setError(null); await load(); }
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!profile) return;
    const form = new FormData(event.currentTarget);
    setBusy(true);
    const r = await send("PATCH", "/api/me/profile", { display_name: form.get("display_name"), bio: form.get("bio"), revision: profile.revisions.profile });
    setBusy(false);
    if (!r.ok) { setError({ message: r.error, field: r.field }); return; }
    onNext();
  }

  return <section className="onboarding-step frame" aria-labelledby="profile-title">
    <h1 id="profile-title" className="auth-title">Set up your profile</h1>
    <p className="intro">How should people know you? You can change all of this later in Settings.</p>
    {error && <p className="notice error" role="alert">{error.message}</p>}
    {!profile ? <p className="loading">Loading your profile…</p> : <form onSubmit={save}>
      <div className="avatar-row">
        <Avatar sizes={profile.avatar} name={profile.display_name || username} size={72} />
        <label className="button quiet small">{profile.avatar ? "Change avatar" : "Upload avatar"}<input className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" disabled={busy} onChange={e => upload(e.target.files?.[0])} /></label>
        <span className="small-text">JPEG, PNG or WebP.</span>
      </div>
      <label className="field" htmlFor="display_name"><span>Display name</span><input id="display_name" name="display_name" defaultValue={profile.display_name} maxLength={32} aria-invalid={error?.field === "display_name" || undefined} /><small>Up to 32 characters. Your @{username} always shows next to it.</small></label>
      <label className="field" htmlFor="bio"><span>Bio</span><textarea id="bio" name="bio" value={bio} onChange={e => setBio(e.target.value)} maxLength={300} rows={4} aria-invalid={error?.field === "bio" || undefined} /><small>{bio.length}/300</small></label>
      <div className="onboarding-actions split">
        <button type="button" className="quiet" onClick={onBack}>Back</button>
        <span className="onboarding-actions">
          <button type="button" className="link-button" onClick={onNext}>I&apos;ll do this later</button>
          <button disabled={busy}>{busy ? "Saving…" : "Continue"}</button>
        </span>
      </div>
    </form>}
  </section>;
}

function FollowStep({ onNext, onBack }: { onNext: (count: number) => void; onBack: () => void }) {
  const [items, setItems] = useState<Chip[] | null>(null);
  const [following, setFollowing] = useState<Set<string>>(new Set());
  const [error, setError] = useState("");
  useEffect(() => { send<{ items: Chip[] }>("GET", "/api/me/suggestions").then(r => setItems(r.ok ? r.data.items : [])); }, []);

  async function toggle(name: string) {
    const on = following.has(name);
    const r = await send(on ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(name)}`);
    if (!r.ok) { setError(r.error); return; }
    setError("");
    setFollowing(prev => { const next = new Set(prev); if (on) next.delete(name); else next.add(name); return next; });
  }

  return <section className="onboarding-step wide" aria-labelledby="follow-title">
    <header className="onboarding-head">
      <h1 id="follow-title">Follow some creators</h1>
      <p>You&apos;ll get an alert when they go live. Live channels and your own side come first.</p>
    </header>
    {error && <p className="notice error" role="alert">{error}</p>}
    {!items ? <p className="loading center">Finding creators…</p> : items.length === 0
      ? <div className="panel empty-follow"><strong>You&apos;re early.</strong><span>Nobody has streamed here yet. Channels show up here once they go live.</span></div>
      : <ul className="follow-grid">{items.map(c => {
        const name = c.username ?? "";
        const on = following.has(name);
        return <li key={name} className={on ? "follow-card on" : "follow-card"}>
          <Avatar sizes={c.avatar} name={c.display_name} size={56} />
          <span className="follow-text">
            <span className="follow-name">{c.display_name}</span>
            <span className="follow-faction"><Crest faction={c.faction ?? null} initial="" size={18} />{factionOf(c.faction)?.name ?? "No side yet"}{c.live && <span className="live-chip">Live</span>}</span>
          </span>
          <button type="button" className={on ? "small" : "quiet small"} aria-pressed={on} onClick={() => toggle(name)}>{on ? "Following" : "Follow"}</button>
        </li>;
      })}</ul>}
    <div className="onboarding-actions split">
      <button type="button" className="quiet" onClick={onBack}>Back</button>
      <span className="onboarding-actions">
        {following.size === 0 && <button type="button" className="link-button" onClick={() => onNext(0)}>Skip for now</button>}
        <button type="button" onClick={() => onNext(following.size)}>{following.size ? `Continue (${following.size} followed)` : "Continue"}</button>
      </span>
    </div>
  </section>;
}

function Ready({ slug, followed, username }: { slug: FactionSlug | null; followed: number; username: string }) {
  const f = factionOf(slug);
  const [verified, setVerified] = useState<boolean | null>(null);
  const [resent, setResent] = useState(false);
  useEffect(() => { send<{ email_verified: boolean }>("GET", "/api/auth/me").then(r => r.ok && setVerified(r.data.email_verified)); }, []);
  async function resend() {
    const r = await send("POST", "/api/auth/email/resend", {});
    setResent(r.ok);
  }
  return <section className="onboarding-step ready frame" aria-labelledby="ready-title">
    {f && <Image src={crestSrc(f.slug)} width={88} height={88} alt="" unoptimized />}
    <h1 id="ready-title" className="auth-title">You&apos;re in!</h1>
    <ul className="ready-chips">
      {f && <li>{f.name} · {f.title}</li>}
      <li>{followed} {followed === 1 ? "channel" : "channels"} followed</li>
    </ul>
    {verified === false && <div className="ready-email">
      <span className="eyebrow">Confirm your email</span>
      <p>We sent you a confirmation link; it works for 24 hours. You can browse and watch now. Chat and going live unlock once you confirm.</p>
      <button type="button" className="quiet small" onClick={resend} disabled={resent}>{resent ? "Email sent again" : "Resend the email"}</button>
    </div>}
    <nav className="ready-links" aria-label="Where to next">
      <Link href="/">Watch a stream</Link>
      <Link href="/studio/stream">Start streaming</Link>
      <Link href={`/${username}`}>Your channel</Link>
      <Link href="/factions">{f ? `${f.name} and the factions` : "The factions"}</Link>
    </nav>
    {/* A full load so the server renders the new faction theme everywhere. */}
    <button type="button" className="primary" onClick={() => window.location.assign("/")}>Enter S.V.E.R</button>
  </section>;
}
