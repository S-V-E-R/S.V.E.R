"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { EmoteImage, type ChannelEmote } from "./Emote";
import { ReportButton, TakeDownLink } from "./Report";
import { SkillArt } from "./SkillArt";
import type { Reward } from "./Rewards";

type Tab = "emotes" | "signature" | "rewards" | "skills";
type Skill = { id: string; name: string; category: string; valor: number; effect: string; enabled: boolean };

/**
 * Everything a viewer can add to chat, in one row under the message box (Mixer's chat bar):
 * Emotes, Rewards (Engagement Valor) and Skills (Valor). Each opens a panel over the bottom of the
 * chat with tiles; the chat column itself stays messages plus the box.
 */
export function ChatDock({ username, account, emotes, onEmote, rewardsVersion, onHighlight, shared }: {
  username: string; account: string | null; emotes: ChannelEmote[]; onEmote: (code: string) => void;
  rewardsVersion: number; onHighlight: (cost: number) => void; shared: boolean;
}) {
  const [tab, setTab] = useState<Tab | null>(null);
  const base: [Tab, string][] = account ? [["emotes", "Emotes"], ["signature", "Signature"]] : [["emotes", "Emotes"]];
  const tabs: [Tab, string][] = shared ? base : [...base, ["rewards", "Rewards"], ["skills", "Skills"]];
  return <div className="chat-dock">
    {tab && <div className="chat-dock-panel panel" role="dialog" aria-label={tabs.find(t => t[0] === tab)?.[1]}>
      <div className="chat-dock-tabs" role="tablist">{tabs.map(([id, label]) => <button key={id} type="button" role="tab" aria-selected={tab === id} className="small quiet" onClick={() => setTab(id)}>{label}</button>)}
        <button type="button" className="small quiet chat-dock-close" aria-label="Close" onClick={() => setTab(null)}>×</button></div>
      {tab === "emotes" && <EmotesTab username={username} account={account} emotes={emotes} onEmote={code => { onEmote(code); setTab(null); }} />}
      {tab === "signature" && <SignatureTab onEmote={code => { onEmote(code); setTab(null); }} />}
      {tab === "rewards" && <RewardsTab username={username} account={account} version={rewardsVersion} onHighlight={cost => { onHighlight(cost); setTab(null); }} />}
      {tab === "skills" && <SkillsTab username={username} account={account} />}
    </div>}
    <div className="chat-dock-bar">{tabs.map(([id, label]) => <button key={id} type="button" className="small quiet" aria-expanded={tab === id} onClick={() => setTab(tab === id ? null : id)}>{label}</button>)}</div>
  </div>;
}

function EmotesTab({ username, account, emotes, onEmote }: { username: string; account: string | null; emotes: ChannelEmote[]; onEmote: (code: string) => void }) {
  if (emotes.length === 0) return <p className="muted">No channel emotes yet.</p>;
  return <ul className="dock-tiles">{emotes.map(emote => <li key={emote.id}>
    {account ? <button type="button" className="dock-tile" onClick={() => onEmote(emote.code)} aria-label={`Insert ${emote.code}`}><EmoteImage emote={emote} /><span>{emote.code}</span></button>
      : <span className="dock-tile"><EmoteImage emote={emote} /><span>{emote.code}</span></span>}
    {account && account.toLowerCase() !== username.toLowerCase() ? <ReportButton label={`Report ${emote.code}`} target={{ target_type: "emote", target_id: emote.id }} /> : <TakeDownLink target={{ target_type: "emote", target_id: emote.id }} />}
  </li>)}</ul>;
}

/** Channel rewards bought with Engagement Valor (docs/SUPPORT.md). Highlight arms the chat box. */
function RewardsTab({ username, account, version, onHighlight }: { username: string; account: string | null; version: number; onHighlight: (cost: number) => void }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [data, setData] = useState<{ rewards: Reward[]; balance: number | null } | null>(null);
  const [active, setActive] = useState<Reward | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ rewards: Reward[]; balance: number | null }>("GET", `${path}/rewards?v=${version}`);
    if (r.ok) setData(r.data); else setNote(r.error);
  }, [path, version]);
  useLoad(load);
  if (!data) return <p className="loading">{note || "Loading…"}</p>;
  const own = !!account && account.toLowerCase() === username.toLowerCase();
  async function redeem(reward: Reward) {
    setBusy(true); setNote("");
    const r = await send("POST", `${path}/rewards/${encodeURIComponent(reward.id)}/redeem`, { id: crypto.randomUUID(), input: reward.prompt ? input : undefined });
    setBusy(false);
    if (r.ok) { setNote(`Redeemed ${reward.name}. The streamer will see it in their queue.`); setActive(null); setInput(""); } else setNote(r.error);
    await load();
  }
  return <>
    <p className="muted small">{data.balance !== null ? <><strong>{data.balance.toLocaleString()}</strong> Engagement Valor, earned here by watching, chatting and following. No cash value.</> : account ? null : <><Link href="/login">Sign in</Link> to earn and spend Engagement Valor.</>}</p>
    {data.rewards.length === 0 ? <p className="muted">This channel has no rewards yet.</p> : <ul className="dock-tiles">{data.rewards.map(reward => {
      const affordable = data.balance !== null && data.balance >= reward.cost;
      const usable = !!account && !own && affordable && !busy;
      return <li key={reward.id}><button type="button" className="dock-tile" disabled={!usable} title={own ? "Your own channel" : !affordable && account ? "Not enough Engagement Valor yet" : undefined}
        onClick={() => { if (reward.kind === "highlight") onHighlight(reward.cost); else { setActive(reward); setInput(""); } }}>
        <span className="dock-tile-name">{reward.name}</span><span className="dock-cost">{reward.cost.toLocaleString()}</span></button></li>;
    })}</ul>}
    {active && <div className="row wrap">{active.prompt && <input aria-label={active.prompt} placeholder={active.prompt} value={input} maxLength={200} onChange={e => setInput(e.target.value)} />}
      <button type="button" className="small" disabled={busy} onClick={() => redeem(active)}>Redeem {active.name}</button><button type="button" className="small quiet" onClick={() => setActive(null)}>Cancel</button></div>}
    {note && <p role="status" className="form-message">{note}</p>}
  </>;
}

/** Skills paid in Purchased Valor: shown on stream and in chat, paid to the streamer like a tribute. */
function SkillsTab({ username, account }: { username: string; account: string | null }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [data, setData] = useState<{ skills: Skill[]; valor: number | null; paused: boolean } | null>(null);
  const [text, setText] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await send<{ skills: Skill[]; valor: number | null; paused: boolean }>("GET", `${path}/skills`);
    if (r.ok) setData(r.data); else setNote(r.error);
  }, [path]);
  useLoad(load);
  async function play(skill: Skill) {
    if (!window.confirm(`Play ${skill.name} for ${skill.valor.toLocaleString()} Valor?`)) return;
    setBusy(true); setNote("");
    const r = await send("POST", `${path}/chat`, { id: crypto.randomUUID(), body: text.trim() || skill.name, skill: skill.id });
    setBusy(false);
    if (!r.ok) { setNote(r.error); return; }
    setText(""); setNote(`${skill.name} played.`);
    setData(d => d && { ...d, valor: d.valor === null ? null : d.valor - skill.valor });
  }
  if (!data) return <p className="loading">{note || "Loading…"}</p>;
  const canPlay = data.valor !== null;
  return <>
    <p className="muted small">{canPlay ? <><strong>{data.valor!.toLocaleString()}</strong> Valor · <Link href="/wallet">Get Valor</Link>. Skills show on stream and in chat and pay the streamer.</> : account ? "Streamers can't play Skills on their own channel." : <><Link href="/login">Sign in</Link> to play Skills.</>}</p>
    {data.paused && <p className="form-message">Effects are paused on this channel right now.</p>}
    {canPlay && <label className="field"><span>Message (optional)</span><input value={text} maxLength={200} onChange={e => setText(e.target.value)} /></label>}
    <ul className="dock-tiles">{data.skills.filter(s => s.enabled).map(s => <li key={s.id}>
      <button type="button" className="dock-tile" disabled={!canPlay || busy || data.paused || (data.valor ?? 0) < s.valor} onClick={() => play(s)}>
        <SkillArt id={s.id} size={40} /><span className="dock-tile-name">{s.name}</span><span className="dock-cost">{s.valor.toLocaleString()}</span></button></li>)}</ul>
    {note && <p role="status" className="form-message">{note}</p>}
  </>;
}

/** Signature emotes of channels the viewer follows; they work in any chat as username/Code. */
function SignatureTab({ onEmote }: { onEmote: (code: string) => void }) {
  const [emotes, setEmotes] = useState<(ChannelEmote & { owner?: string })[] | null>(null);
  const load = useCallback(async () => {
    const r = await send<{ emotes: (ChannelEmote & { owner?: string })[] }>("GET", "/api/me/signature-emotes");
    setEmotes(r.ok ? r.data.emotes : []);
  }, []);
  useLoad(load);
  if (!emotes) return <p className="loading">Loading…</p>;
  if (emotes.length === 0) return <p className="muted">Channels you follow haven&apos;t chosen a signature emote yet.</p>;
  return <ul className="dock-tiles">{emotes.map(emote => <li key={emote.id}>
    <button type="button" className="dock-tile" onClick={() => onEmote(emote.code)} aria-label={`Insert ${emote.code}`} title={`@${emote.owner}'s signature emote`}><EmoteImage emote={emote} /><span>{emote.code}</span></button>
  </li>)}</ul>;
}
