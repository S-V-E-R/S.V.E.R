"use client";
import Link from "next/link";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { send, useLoad, type Result } from "../lib/client-api";
import { EmoteImage, providerNames, type ChannelEmote } from "./Emote";
import { ReportForm, TakeDownLink } from "./Report";
import { SkillArt } from "./SkillArt";
import type { Reward } from "./Rewards";

type Tab = "emotes" | "signature" | "rewards" | "skills";
type Skill = { id: string; name: string; category: string; valor: number; effect: string; enabled: boolean; custom?: boolean; image?: string | null };

/** Tiles per page: 3 rows of 4, so a panel never needs a scrollbar (Joe, October 11, 2026). */
const PAGE = 12;

/** One request id per intent (`key`): a retry after a lost response reuses it, so the server answers
 *  with the first result instead of charging twice. Any answer clears it except a network failure or a 5xx. */
function useRequestId() {
  const pending = useRef<{ id: string; key: string } | null>(null);
  return {
    id(key: string) { if (pending.current?.key !== key) pending.current = { id: crypto.randomUUID(), key }; return pending.current.id; },
    settle(result: Result<unknown>) { if (result.ok || (result.status >= 400 && result.status < 500)) pending.current = null; },
  };
}

/** Emotes whose code contains `query`: exact matches, then prefix, then the rest; S.V.E.R emotes before
 *  7TV, BTTV and FFZ within each. */
function matching(emotes: ChannelEmote[], query: string) {
  const q = query.trim().toLowerCase();
  if (!q) return emotes;
  const rank = (e: ChannelEmote) => { const code = e.code.toLowerCase(); return (code === q ? 0 : code.startsWith(q) ? 2 : 4) + (e.provider ? 1 : 0); };
  return emotes.filter(e => e.code.toLowerCase().includes(q)).sort((a, b) => rank(a) - rank(b));
}

function EmoteSearch({ query, onQuery }: { query: string; onQuery: (query: string) => void }) {
  return <input type="search" className="dock-message" aria-label="Search emotes" placeholder="Search emotes" value={query} maxLength={60} autoFocus onChange={e => onQuery(e.target.value)} />;
}

/**
 * Everything a viewer can add to chat, in one row under the message box (Mixer's chat bar):
 * Emotes, Signature, Rewards (Engagement Valor) and Skills (Valor). Each opens a panel over the
 * bottom of the chat with a fixed grid of tiles, paged instead of scrolled. `dm`: direct messages
 * show only the Signature tab (no channel, so no channel emotes, rewards or skills).
 */
export function ChatDock({ username, account, emotes, outside = [], onOutsideReported, onEmote, rewardsVersion, onHighlight, shared, dm }: {
  username: string; account: string | null; emotes: ChannelEmote[]; outside?: ChannelEmote[]; onOutsideReported?: () => void; onEmote: (code: string) => void;
  rewardsVersion: number; onHighlight: (cost: number) => void; shared: boolean; dm?: boolean;
}) {
  const [tab, setTab] = useState<Tab | null>(null);
  const opener = useRef<HTMLButtonElement | null>(null);
  const head = useRef<HTMLElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  // On open, focus lands on the heading unless a search box already took it.
  useEffect(() => { if (tab && !panel.current?.contains(document.activeElement)) head.current?.focus(); }, [tab]);
  const close = () => { setTab(null); opener.current?.focus(); };
  const base: [Tab, string][] = account ? [["emotes", "Emotes"], ["signature", "Signature"]] : [["emotes", "Emotes"]];
  const tabs: [Tab, string][] = dm ? [["signature", "Signature"]] : shared ? base : [...base, ["rewards", "Rewards"], ["skills", "Skills"]];
  const insert = (code: string) => { onEmote(code); setTab(null); };
  return <div className="chat-dock">
    {tab && <div ref={panel} className="chat-dock-panel panel" role="dialog" aria-label={tabs.find(t => t[0] === tab)?.[1]} onKeyDown={e => { if (e.key === "Escape") { e.stopPropagation(); close(); } }}>
      <div className="chat-dock-head"><strong ref={head} tabIndex={-1}>{tabs.find(t => t[0] === tab)?.[1]}</strong>
        <button type="button" className="small quiet chat-dock-close" aria-label="Close" onClick={close}>×</button></div>
      {tab === "emotes" && <EmotesTab key="emotes" username={username} account={account} emotes={[...[...emotes].sort((a, b) => (a.tier ?? 0) - (b.tier ?? 0)), ...outside]} onEmote={insert} onOutsideReported={onOutsideReported} />}
      {tab === "signature" && <SignatureTab key="signature" onEmote={insert} />}
      {tab === "rewards" && <RewardsTab key="rewards" username={username} account={account} version={rewardsVersion} onHighlight={cost => { onHighlight(cost); close(); }} />}
      {tab === "skills" && <SkillsTab key="skills" username={username} account={account} />}
    </div>}
    <div className="chat-dock-bar" style={{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }}>{tabs.map(([id, label]) => <button key={id} type="button" className="small quiet" aria-expanded={tab === id} onClick={e => { opener.current = e.currentTarget; setTab(tab === id ? null : id); }}>{label}</button>)}</div>
  </div>;
}

/** A 4-wide grid of tiles, 12 to a page, with ‹ › when there are more. */
function Tiles<T>({ items, children }: { items: T[]; children: (item: T) => ReactNode }) {
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(items.length / PAGE));
  const at = Math.min(page, pages - 1);
  return <>
    <ul className="dock-tiles">{items.slice(at * PAGE, at * PAGE + PAGE).map(children)}</ul>
    {pages > 1 && <div className="dock-pager">
      <button type="button" className="small quiet" disabled={at === 0} aria-label="Previous page" onClick={() => setPage(at - 1)}>‹</button>
      <span className="small-text">{at + 1} of {pages}</span>
      <button type="button" className="small quiet" disabled={at === pages - 1} aria-label="Next page" onClick={() => setPage(at + 1)}>›</button>
    </div>}
  </>;
}

/**
 * The channel's own emotes, then its 7TV, BTTV and FFZ emotes. Reporting is one mode for the whole
 * tab rather than a link under every tile: switch it on, then pick the emote.
 */
function EmotesTab({ username, account, emotes, onEmote, onOutsideReported }: { username: string; account: string | null; emotes: ChannelEmote[]; onEmote: (code: string) => void; onOutsideReported?: () => void }) {
  const [reporting, setReporting] = useState(false);
  const [chosen, setChosen] = useState<ChannelEmote | null>(null);
  const [message, setMessage] = useState("");
  const [query, setQuery] = useState("");
  if (emotes.length === 0) return <p className="muted">No channel emotes yet.</p>;
  const found = matching(emotes, query);
  const own = account?.toLowerCase() === username.toLowerCase();
  async function reportOutside(emote: ChannelEmote) {
    const r = await send("POST", `/api/channels/${encodeURIComponent(username)}/outside-emotes/${emote.provider}/${encodeURIComponent(emote.id)}/report`);
    setMessage(r.ok ? `${emote.code} is hidden in this chat until the streamer reviews it.` : r.error);
    setChosen(null); setReporting(false);
    if (r.ok) onOutsideReported?.();
  }
  function pick(emote: ChannelEmote) {
    if (!reporting) { if (account) onEmote(emote.code); return; }
    setMessage("");
    if (emote.provider && account) { if (window.confirm(`Report ${emote.code}? It's hidden in this chat until the streamer reviews it.`)) void reportOutside(emote); }
    else setChosen(emote);
  }
  return <>
    <EmoteSearch query={query} onQuery={setQuery} />
    {found.length === 0 ? <p className="muted">No emotes match.</p> : <Tiles key={query} items={found}>{emote => {
      // Tier emotes stay clickable: the channel's own team can use every tier.
      const tier = emote.tier ? ` (Tier ${emote.tier} subscribers)` : "";
      return <li key={`${emote.provider ?? "sver"}:${emote.id}`}>
        <button type="button" className={`dock-tile${reporting ? " reporting" : ""}`} disabled={!account && !reporting} onClick={() => pick(emote)}
          aria-label={`${reporting ? "Report" : "Insert"} ${emote.code}${tier}`} title={emote.provider ? `${emote.code} (${providerNames[emote.provider]})` : `${emote.code}${tier}`}>
          <EmoteImage emote={emote} /><span>{emote.code}</span>{emote.tier ? <span className="dock-cost">Tier {emote.tier}</span> : null}
        </button>
      </li>;
    }}</Tiles>}
    {!own && <div className="dock-foot">
      <button type="button" className="link-button" aria-pressed={reporting} onClick={() => { setReporting(!reporting); setChosen(null); setMessage(""); }}>{reporting ? "Cancel report" : "Report an emote"}</button>
      {reporting && !chosen && <span className="small-text">Pick the emote to report.</span>}
    </div>}
    {chosen && (account ? <ReportForm target={{ target_type: "emote", target_id: chosen.id }} onDone={() => { setChosen(null); setReporting(false); }} />
      : <TakeDownLink target={{ target_type: "emote", target_id: chosen.id }} />)}
    {message && <p role="status" className="small-text">{message}</p>}
  </>;
}

/** Channel rewards bought with Engagement Valor (docs/SUPPORT.md). Highlight arms the chat box. */
function RewardsTab({ username, account, version, onHighlight }: { username: string; account: string | null; version: number; onHighlight: (cost: number) => void }) {
  const path = `/api/channels/${encodeURIComponent(username)}`;
  const [data, setData] = useState<{ rewards: Reward[]; balance: number | null } | null>(null);
  const [active, setActive] = useState<Reward | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const requestId = useRequestId();
  const load = useCallback(async () => {
    const r = await send<{ rewards: Reward[]; balance: number | null }>("GET", `${path}/rewards?v=${version}`);
    if (r.ok) setData(r.data); else setNote(r.error);
  }, [path, version]);
  useLoad(load);
  if (!data) return <p className="loading">{note || "Loading…"}</p>;
  const own = !!account && account.toLowerCase() === username.toLowerCase();
  async function redeem(reward: Reward) {
    setBusy(true); setNote("");
    const typed = reward.prompt ? input : undefined;
    const r = await send("POST", `${path}/rewards/${encodeURIComponent(reward.id)}/redeem`, { id: requestId.id(JSON.stringify([reward.id, typed])), input: typed });
    requestId.settle(r);
    setBusy(false);
    if (r.ok) { setNote(`Redeemed ${reward.name}. The streamer will see it in their queue.`); setActive(null); setInput(""); } else setNote(r.error);
    await load();
  }
  return <>
    <p className="small-text">{data.balance !== null ? <><strong>{data.balance.toLocaleString()}</strong> Engagement Valor, earned here by watching, chatting and following. No cash value.</> : account ? null : <><Link href="/login">Sign in</Link> to earn and spend Engagement Valor.</>}</p>
    {data.rewards.length === 0 ? <p className="muted">This channel has no rewards yet.</p> : <Tiles items={data.rewards}>{reward => {
      const affordable = data.balance !== null && data.balance >= reward.cost;
      const usable = !!account && !own && affordable && !busy;
      return <li key={reward.id}><button type="button" className="dock-tile" disabled={!usable} title={own ? "Your own channel" : !affordable && account ? "Not enough Engagement Valor yet" : undefined}
        onClick={() => { if (reward.kind === "highlight") onHighlight(reward.cost); else { setActive(reward); setInput(""); } }}>
        <span className="dock-tile-name">{reward.name}</span><span className="dock-cost">{reward.cost.toLocaleString()}</span></button></li>;
    }}</Tiles>}
    {active && <div className="dock-foot">{active.prompt && <input aria-label={active.prompt} placeholder={active.prompt} value={input} maxLength={200} onChange={e => setInput(e.target.value)} />}
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
  const requestId = useRequestId();
  const load = useCallback(async () => {
    const r = await send<{ skills: Skill[]; valor: number | null; paused: boolean }>("GET", `${path}/skills`);
    if (r.ok) setData(r.data); else setNote(r.error);
  }, [path]);
  useLoad(load);
  async function play(skill: Skill) {
    if (!window.confirm(`Play ${skill.name} for ${skill.valor.toLocaleString()} Valor?`)) return;
    setBusy(true); setNote("");
    const body = text.trim() || skill.name;
    const r = await send("POST", `${path}/chat`, { id: requestId.id(JSON.stringify([skill.id, body])), body, skill: skill.id });
    requestId.settle(r);
    setBusy(false);
    if (!r.ok) { setNote(r.error); return; }
    setText(""); setNote(`${skill.name} played.`);
    setData(d => d && { ...d, valor: d.valor === null ? null : d.valor - skill.valor });
  }
  if (!data) return <p className="loading">{note || "Loading…"}</p>;
  const canPlay = data.valor !== null;
  return <>
    <p className="small-text">{canPlay ? <><strong>{data.valor!.toLocaleString()}</strong> Valor · <Link href="/wallet">Get Valor</Link>. Skills show on stream and in chat and pay the streamer.</> : account ? "Streamers can't play Skills on their own channel." : <><Link href="/login">Sign in</Link> to play Skills.</>}</p>
    {data.paused && <p className="form-message">Effects are paused on this channel right now.</p>}
    {canPlay && <input className="dock-message" aria-label="Message with the Skill (optional)" placeholder="Message (optional)" value={text} maxLength={200} onChange={e => setText(e.target.value)} />}
    <Tiles items={data.skills.filter(s => s.enabled)}>{s => <li key={s.id}>
      <button type="button" className="dock-tile" disabled={!canPlay || busy || data.paused || (data.valor ?? 0) < s.valor} onClick={() => play(s)}>
        {/* eslint-disable-next-line @next/next/no-img-element -- the streamer's uploaded Skill image, already resized */}
        {s.image ? <img src={s.image} alt="" width={32} height={32} className="skill-art" /> : <SkillArt id={s.custom ? "custom" : s.id} size={32} />}<span className="dock-tile-name">{s.name}</span><span className="dock-cost">{s.valor.toLocaleString()}</span></button></li>}</Tiles>
    {note && <p role="status" className="form-message">{note}</p>}
  </>;
}

/** Signature emotes of channels the viewer follows; they work in any chat as username/Code. */
function SignatureTab({ onEmote }: { onEmote: (code: string) => void }) {
  const [emotes, setEmotes] = useState<ChannelEmote[] | null>(null);
  const [query, setQuery] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ emotes: ChannelEmote[] }>("GET", "/api/me/signature-emotes");
    setEmotes(r.ok ? r.data.emotes : []);
  }, []);
  useLoad(load);
  if (!emotes) return <p className="loading">Loading…</p>;
  if (emotes.length === 0) return <p className="muted">Channels you follow haven&apos;t chosen a signature emote yet.</p>;
  const found = matching(emotes, query);
  return <>
    <EmoteSearch query={query} onQuery={setQuery} />
    {found.length === 0 ? <p className="muted">No emotes match.</p> : <Tiles key={query} items={found}>{emote => <li key={emote.id}>
      <button type="button" className="dock-tile" onClick={() => onEmote(emote.code)} aria-label={`Insert ${emote.code}`} title={`@${emote.owner}'s signature emote`}><EmoteImage emote={emote} /><span>{emote.code}</span></button>
    </li>}</Tiles>}
  </>;
}
