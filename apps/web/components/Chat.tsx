"use client";
import { FounderMark, SupporterBadge, type SupporterTier } from "./Supporter";
import Link from "next/link";
import { LOYALTY } from "./Loyalty";
import { FormEvent, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { send, useLoad, usePendingId, useSignInElsewhere } from "../lib/client-api";
import { ReportButton, TakeDownLink } from "./Report";
import { PlayerCardButton } from "./UserChip";
import { Crest } from "./FactionIdentity";
import { GuildChatBadge } from "./Guilds";
import { CREATOR_TIERS, type Chip } from "../lib/types";
import { EmoteImage, type ChannelEmote } from "./Emote";
import { BanIcon, ClockIcon, TrashIcon } from "./shell/Icons";
import { SignatureEmote } from "./SignatureEmote";
import { ChatDock } from "./ChatDock";
import { GifImage, GifPicker, type Gif, type GifAccess } from "./GifPicker";
import "../styles/teams.css";

type Reply = { id: string; username: string | null; body: string | null };
/** `protected` and `moderator` are live (the stored `role` is from send time); `arrived` is stamped here for the overlay fade. */
export type Message = { id: string; seq: number; author: Chip; body: string; created_at: string; role: "owner" | "moderator" | "staff" | null; mentions: string[]; reply: Reply | null; origin?: string | null; tribute?: number | null; sub?: { tier: number; months: number } | null; highlighted?: boolean; creator_tier?: number | null; skill?: string | null; loyalty?: number; outside?: Outside | null; gif?: Gif | null; supporter?: { tier: SupporterTier; months: number; founder: number | null } | null; protected?: boolean; moderator?: boolean; arrived?: number };
/** A message from a linked platform (docs/LINKED_CHAT.md): always badged, never a S.V.E.R account. */
type Outside = { platform: string; sender_id: string; name: string; login: string; role: string | null; url: string | null };
const PLATFORM_NAMES: Record<string, string> = { twitch: "Twitch", youtube: "YouTube", kick: "Kick" };
/** Subscriber badge milestones: 1, 3, 6, 9 and 12 months, then each further year (docs/SUPPORT.md). */
export function subBadge(months: number) {
  if (months >= 24) return `${Math.floor(months / 12)} years`;
  if (months >= 12) return "1 year";
  const step = [9, 6, 3].find(m => months >= m) ?? 1;
  return `${step} ${step === 1 ? "month" : "months"}`;
}
type Snapshot = { messages: Message[]; pinned: Message | null; emotes: ChannelEmote[]; followers_only_until?: string | null; subs_only?: boolean; allow_signatures?: boolean; gifs?: GifAccess };
/** Another channel's signature emote, written username/Code (docs/CHANNEL_ADDITIONS.md "Signature emote"). */
const SIGNATURE = /(?<!\S)[A-Za-z0-9_]{3,25}\/[A-Za-z0-9]{3,20}(?!\S)/g;
const BOARD_EVENTS = new Set(["board_effect", "board_input", "board", "poll", "counters", "rally", "surge", "board_state", "board_result"]);
type BoardEvent = { type: "board_effect" | "board_input" | "board" | "poll" | "counters" | "rally" | "surge" | "board_state" | "board_result"; source?: string };
// `outside`: the channel's 7TV/BTTV/FFZ set changed too. `source`: a co-stream member's own event, named by its channel.
type Event = ({ type: "snapshot" } & Snapshot) | { type: "emotes"; emotes?: ChannelEmote[]; outside?: boolean; allow_signatures?: boolean } | { type: "pin"; pinned: Message | null } | { type: "message"; message: Message } | { type: "ack"; id: string; message: Message } | { type: "error"; id?: string; message: string; retry?: number } | { type: "delete"; id: string } | { type: "raid"; raid: unknown; source?: string } | { type: "raid_cancelled"; id: string; source?: string } | { type: "system"; text: string } | { type: "protect"; until: string | null } | { type: "subs_only"; on: boolean } | BoardEvent;

const time = (iso: string) => new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
const NONE: ChannelEmote[] = [];

/** Reconnect delay: capped, jittered backoff that never gives up, so a deploy's reconnect herd spreads
 *  out (and stays under the per-network connect limit); a server resync (4000) retries at once. */
export const reconnectDelay = (attempt: number, code: number) => code === 4000 ? Math.random() * 1000 : Math.min(30000, 1000 * 2 ** attempt) * (0.5 + Math.random());

/** Emote codes for MessageBody: exact codes, plus signature emotes (username/Code) by lowercase. The
 *  first list entry wins a clash, so a channel's own emotes come before signatures and outside sets. */
export type EmoteLookup = { exact: Map<string, ChannelEmote>; slash: Map<string, ChannelEmote> };
export function emoteLookup(emotes: ChannelEmote[]): EmoteLookup {
  const exact = new Map<string, ChannelEmote>(), slash = new Map<string, ChannelEmote>();
  for (const e of emotes) {
    if (!exact.has(e.code)) exact.set(e.code, e);
    const folded = e.code.toLowerCase();
    if (e.code.includes("/") && !slash.has(folded)) slash.set(folded, e);
  }
  return { exact, slash };
}

/** Signature emotes in these messages, looked up as they appear and again every 90 seconds (and on
 *  `refresh`), so a removed or rejected one stops showing in open chats, pop-outs and overlays. */
export function useSignatures(messages: Message[], allow: boolean) {
  const [signatures, setSignatures] = useState<ChannelEmote[]>([]);
  // Lowercase code → the code as written (emote codes are case-sensitive).
  const asked = useRef(new Map<string, string>());
  const lookup = useCallback(async (codes: string[]) => {
    const found: ChannelEmote[] = [];
    for (let i = 0; i < codes.length; i += 30) {
      const r = await send<{ emotes: ChannelEmote[] }>("GET", `/api/emotes/signatures?codes=${encodeURIComponent(codes.slice(i, i + 30).join(","))}`);
      if (!r.ok) return null;
      found.push(...r.data.emotes);
    }
    return found;
  }, []);
  useEffect(() => {
    if (!allow) return;
    const present = new Map(messages.flatMap(m => m.body.match(SIGNATURE) ?? []).map(code => [code.toLowerCase(), code]));
    for (const code of asked.current.keys()) if (!present.has(code)) asked.current.delete(code);
    const wanted = [...present].filter(([folded]) => !asked.current.has(folded)).slice(0, 30);
    if (!wanted.length) return;
    wanted.forEach(([folded, code]) => asked.current.set(folded, code));
    void lookup(wanted.map(([, code]) => code)).then(found => { if (found?.length) setSignatures(known => [...known, ...found]); });
  }, [messages, allow, lookup]);
  const refresh = useCallback(async () => { const found = await lookup([...asked.current.values()]); if (found) setSignatures(found); }, [lookup]);
  useEffect(() => {
    if (!allow) return;
    const timer = setInterval(() => { if (!document.hidden) void refresh(); }, 90000);
    return () => clearInterval(timer);
  }, [allow, refresh]);
  return [signatures, refresh] as const;
}

/** The 7TV, BTTV and FFZ emotes a channel shows (served by S.V.E.R); a new `version` reloads them. */
export function useOutsideEmotes(username: string | null, version = 0) {
  const [got, setGot] = useState<{ username: string; emotes: ChannelEmote[] } | null>(null);
  useEffect(() => {
    if (!username) return;
    let live = true;
    void send<{ emotes: ChannelEmote[] }>("GET", `/api/channels/${encodeURIComponent(username)}/outside-emotes`).then(r => { if (live && r.ok) setGot({ username, emotes: r.data.emotes ?? [] }); });
    return () => { live = false; };
  }, [username, version]);
  return got && got.username === username ? got.emotes : NONE;
}

/** Slow mode and timeouts: the server's `retry` seconds, counted down; Send stays off until zero. */
export function useRetryWait() {
  const [wait, setWait] = useState<{ left: number; total: number } | null>(null);
  useEffect(() => {
    if (!wait) return;
    const timer = setTimeout(() => setWait(w => w && w.left > 1 ? { ...w, left: w.left - 1 } : null), 1000);
    return () => clearTimeout(timer);
  }, [wait]);
  const start = useCallback((seconds?: number) => { if (seconds && seconds > 0) setWait({ left: Math.ceil(seconds), total: Math.ceil(seconds) }); }, []);
  return [wait, start] as const;
}
/** Read out once (the total); the visible count ticks without being announced every second. */
export function RetryNotice({ wait }: { wait: { left: number; total: number } | null }) {
  return wait && <p role="status" className="muted small chat-retry"><span className="sr-only">You can send again in {wait.total} seconds.</span><span aria-hidden="true">Send again in {wait.left}s</span></p>;
}

/** Replies to a deleted message quote it as "Message deleted". */
export const redactReply = (id: string) => (m: Message): Message => m.reply?.id === id ? { ...m, reply: { id, username: null, body: null } } : m;

export const MessageBody = memo(function MessageBody({ message, account, lookup }: { message: Message; account: string | null; lookup: EmoteLookup }) {
  if (message.gif) return <GifImage gif={message.gif} alt={message.body.replace(/^GIF: /, "")} />;
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  for (const match of message.body.matchAll(/(?<!\S)[A-Za-z0-9_]{3,25}\/[A-Za-z0-9]{3,20}(?!\S)|(?<!\S)[A-Za-z0-9_]{1,40}(?!\S)|(?<![\p{L}\p{N}_@])@([A-Za-z0-9_]{3,25})(?![\p{L}\p{N}_])|(?<!\S)[^\s@]{0,39}[^\sA-Za-z0-9_@][^\s@]{0,39}(?!\S)/gu)) {
    // Signature emotes match their username case-insensitively; channel emote codes are exact.
    const emote = !match[1] && (lookup.exact.get(match[0]) ?? (match[0].includes("/") ? lookup.slash.get(match[0].toLowerCase()) : undefined));
    const name = match[1] && message.mentions.find(name => name.toLowerCase() === match[1].toLowerCase());
    if (!name && !emote) continue;
    parts.push(message.body.slice(cursor, match.index));
    if (emote) parts.push(emote.owner ? <SignatureEmote key={match.index} emote={emote} owner={emote.owner} /> : <EmoteImage key={match.index} emote={emote} />);
    else if (name) {
      const link = <Link href={`/${name}`}>{match[0]}</Link>;
      parts.push(name.toLowerCase() === account?.toLowerCase() ? <mark key={match.index} className="chat-mention">{link}</mark> : <span key={match.index}>{link}</span>);
    }
    cursor = match.index + match[0].length;
  }
  parts.push(message.body.slice(cursor));
  return <>{message.reply && <blockquote className="chat-quote">{message.reply.body === null ? "Message deleted" : <>{message.reply.username && <strong>@{message.reply.username}: </strong>}{message.reply.body}</>}</blockquote>}<span className="chat-body">{parts}</span></>;
});

const ROLES = { owner: "Broadcaster", moderator: "Moderator", staff: "Staff" } as const;
/** Crest, guild, faction-coloured name and badges. `host`: in a co-stream the other owners read
 *  "Co-streamer". `card`: the name opens the player card (off on the read-only overlay). */
function Identity({ m, host, card = true }: { m: Message; host?: string; card?: boolean }) {
  const data = { faction: m.author.faction ?? undefined, style: m.author.name_style ?? undefined, supporter: m.supporter?.tier };
  return <>
    {m.author.faction && <Crest faction={m.author.faction} size={14} />}{" "}
    {m.author.guild && <GuildChatBadge guild={m.author.guild} />}
    {card && m.author.username ? <PlayerCardButton username={m.author.username} className="chip-button faction-name chat-name" data={data} title={`${LOYALTY[m.loyalty ?? 0]} in this channel`}><strong>{m.author.display_name}</strong></PlayerCardButton>
      : <strong className="faction-name chat-name" data-faction={data.faction} data-style={data.style} data-supporter={data.supporter}>{m.author.display_name}</strong>}
    {m.creator_tier ? <a href="/help/creator-tiers" target="_blank" rel="noopener" className="badge tier-badge" title={`${CREATOR_TIERS[m.creator_tier]} creator tier: see every tier`}>{CREATOR_TIERS[m.creator_tier]}</a> : null}
    {m.supporter && <SupporterBadge tier={m.supporter.tier} monthsSupported={m.supporter.months} />}
    {m.supporter?.founder ? <FounderMark number={m.supporter.founder} /> : null}
    {m.sub && <span className="badge sub-badge" title={`Tier ${m.sub.tier} subscriber`}>{subBadge(m.sub.months)}</span>}
    {m.origin && <span className="badge magnet-badge" title="Sent from MAGNet">MAGNet</span>}
    {m.role && <span className="badge">{m.role === "owner" && host && m.author.username?.toLowerCase() !== host.toLowerCase() ? "Co-streamer" : ROLES[m.role]}</span>}
  </>;
}

/** One S.V.E.R chat line, shared by channel chat and MAGNet chat. `tools` go before the time (quick
 *  moderation); `children` after the message (actions, report). */
export function ChatLine({ m, account, lookup, host, tools, children }: { m: Message; account: string | null; lookup: EmoteLookup; host?: string; tools?: React.ReactNode; children?: React.ReactNode }) {
  return <li className={[m.tribute && "chat-tribute", m.highlighted && "chat-highlight"].filter(Boolean).join(" ") || undefined}>
    {m.tribute && <span className="badge tribute-badge">{m.tribute.toLocaleString()} Valor</span>}
    {m.skill && <span className="badge skill-badge">{m.skill}</span>}
    {tools}
    <span className="muted">{time(m.created_at)}</span>{" "}
    <Identity m={m} host={host} />: <MessageBody message={m} account={account} lookup={lookup} />
    {children}
  </li>;
}

/**
 * Channel chat. Live over the same-origin WebSocket, which reconnects for as long as the page is open;
 * while it is down the chat polls history and sends over HTTPS, which run the same server checks.
 */
/**
 * `variant`: the channel page ("full", with a Pop out button), the pop-out window ("popout"), an OBS
 * dock ("dock": no header, compact) or a read-only OBS overlay ("overlay": messages fade after `fade`
 * seconds). See docs/CHANNEL_ADDITIONS.md "Pop-out chat".
 */
export function Chat({ username, account, squad, variant = "full", fade = 30, top }: { username: string; account: string | null; squad?: string; variant?: "full" | "popout" | "dock" | "overlay"; fade?: number; top?: React.ReactNode }) {
  const [clock, setClock] = useState(() => Date.now());
  // A chat dock signed out in OBS picks up a sign-in made in another dock.
  useSignInElsewhere(variant === "dock" && !account);
  useEffect(() => {
    if (variant !== "overlay") return;
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [variant]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  // The socket send awaiting its ack, on the socket it went out on (a second Enter meanwhile is ignored).
  const sending = useRef<{ id: string; body: string; ws: WebSocket } | null>(null);
  // One client id per send, reused on a retry so it can't post or charge twice.
  const pending = usePendingId();
  const gifPending = usePendingId();
  const [wait, startWait] = useRetryWait();
  const [error, setError] = useState("");
  const [mode, setMode] = useState<"connecting" | "live" | "polling">("connecting");
  const [role, setRole] = useState<string | null>(null);
  const [restrictions, setRestrictions] = useState<{ user: Chip; kind: string; until: string | null }[]>([]);
  const [pinned, setPinned] = useState<Message | null>(null);
  const [emotes, setEmotes] = useState<ChannelEmote[]>([]);
  const [emotesVersion, setEmotesVersion] = useState(0);
  const [allowSignatures, setAllowSignatures] = useState(true);
  const [gifs, setGifs] = useState<GifAccess>(null);
  const [picking, setPicking] = useState(false);
  const [reply, setReply] = useState<Reply | null>(null);
  const [outsideReply, setOutsideReply] = useState<{ id: string; platform: string; name: string } | null>(null);
  // The streamer's connected linked platforms, and the one the next message goes to ("" = S.V.E.R only).
  const [platforms, setPlatforms] = useState<string[]>([]);
  const [postOn, setPostOn] = useState("");
  const [busy, setBusy] = useState(false);
  // Valor to pay with the next message (docs/SUPPORT.md "Purchased Valor"); null when off.
  const [tribute, setTribute] = useState<number | null>(null);
  // Engagement Valor cost of highlighting the next message; null when off. The version reloads the balance after paying.
  const [highlight, setHighlight] = useState<number | null>(null);
  const [rewardsVersion, setRewardsVersion] = useState(0);
  const [notice, setNotice] = useState("");
  // Shield mode: followers-only chat (and its floors) with an end time; moderators get a one-click prompt on a spike.
  const [followersOnly, setFollowersOnly] = useState<string | null>(null);
  const [subsOnly, setSubsOnly] = useState(false);
  const [spike, setSpike] = useState(false);
  const canPin = !squad && (role === "owner" || role === "moderator");
  const socket = useRef<WebSocket | null>(null);
  const list = useRef<HTMLOListElement>(null);
  const follow = useRef(true);
  // The newest seq the reader has seen at the bottom; only newer messages show "New messages below".
  const newestSeen = useRef(0);
  // The backlog loads silently; the list announces new messages only after the first load.
  const loaded = useRef(false);
  const [live, setLive] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const path = squad ? `/api/squads/${encodeURIComponent(squad)}/chat` : `/api/channels/${encodeURIComponent(username)}/chat`;
  const host = squad ? username : undefined;

  // Deduplicates by ID in server order: the latest 100 while following, up to 500 while the reader is
  // scrolled up (a reload then merges in too, so their scrollback stays). Each message is stamped with
  // when it arrived here; a backlog keeps its relative ages, so the overlay fade never uses the PC clock.
  const merge = useCallback((incoming: Message[], replace = false) => {
    if (replace) loaded.current = true;
    const now = Date.now();
    const newest = Math.max(0, ...incoming.map(m => Date.parse(m.created_at)));
    setMessages(current => {
      const arrived = new Map(current.map(m => [m.id, m.arrived]));
      const byId = new Map((replace && follow.current ? [] : current).map(m => [m.id, m]));
      for (const m of incoming) byId.set(m.id, { ...m, arrived: arrived.get(m.id) ?? (replace ? now - (newest - Date.parse(m.created_at)) : now) });
      return [...byId.values()].sort((a, b) => a.seq - b.seq).slice(follow.current ? -100 : -500);
    });
  }, []);

  const remove = useCallback((id: string) => {
    const redact = redactReply(id);
    setMessages(current => current.filter(m => m.id !== id).map(redact));
    setPinned(current => current?.id === id ? null : current && redact(current));
    setReply(current => current?.id === id ? null : current);
  }, []);

  // The server has the socket send (ack, or found in a reload): clear the draft unless it was edited since.
  const ack = useCallback((id: string) => {
    const s = sending.current;
    if (s?.id !== id) return;
    setDraft(d => d === s.body ? "" : d); setReply(null); sending.current = null; pending.clear();
  }, [pending]);

  const withPin = useMemo(() => pinned ? [...messages, pinned] : messages, [messages, pinned]);
  const [signatures, refreshSignatures] = useSignatures(withPin, allowSignatures);
  const outside = useOutsideEmotes(squad ? null : username, emotesVersion);
  const shown = useMemo(() => emoteLookup([...emotes, ...(allowSignatures ? signatures : []), ...outside]), [emotes, signatures, outside, allowSignatures]);
  // Twitch/Kick/YouTube lines render only the channel's mirrored 7TV/BTTV/FFZ set (docs/LINKED_CHAT.md).
  const outsideShown = useMemo(() => emoteLookup(outside), [outside]);

  // Owner, moderators and staff get per-message actions; the server enforces every permission.
  const loadRole = useCallback(async () => {
    if (!account) return;
    const r = await send<{ role: string; spike?: boolean; followers_only_until?: string | null; restrictions?: { user: Chip; kind: string; until: string | null }[] }>("GET", `${path}/moderation`);
    if (r.ok) { setRole(r.data.role); setSpike(!!r.data.spike); setFollowersOnly(r.data.followers_only_until ?? null); setRestrictions(r.data.restrictions ?? []); }
    else setRole(null);
  }, [account, path]);
  useLoad(loadRole);
  useEffect(() => {
    if (!role) return;
    const timer = setInterval(() => { if (!document.hidden) void loadRole(); }, 15000);
    return () => clearInterval(timer);
  }, [role, loadRole]);
  // The streamer's own chat says when a linked platform's chat is down or waiting (docs/LINKED_CHAT.md),
  // in shared co-stream chat too; never on the OBS overlay, which goes out on the video.
  const [linkedDown, setLinkedDown] = useState<string[]>([]);
  useEffect(() => {
    if (role !== "owner" || variant === "overlay") return;
    const load = async () => {
      const r = await send<{ accounts: { platform: string; enabled: boolean; status: string; detail: string | null }[] }>("GET", "/api/me/linked-chat/status");
      if (!r.ok) return;
      setLinkedDown(r.data.accounts.filter(a => a.enabled && (a.status === "reconnecting" || a.status === "revoked" || (a.status === "connecting" && a.detail))).map(a => a.detail ?? `${PLATFORM_NAMES[a.platform] ?? a.platform} chat isn't connected.`));
      const connected = r.data.accounts.filter(a => a.enabled && a.status === "connected").map(a => a.platform);
      setPlatforms(connected);
      setPostOn(p => connected.includes(p) ? p : "");
    };
    void load();
    const timer = setInterval(() => { if (!document.hidden) void load(); }, 30000);
    return () => clearInterval(timer);
  }, [role, variant]);
  // Followers-only chat always ends by itself; clear the banner at its end time.
  useEffect(() => {
    if (!followersOnly) return;
    const timer = setTimeout(() => setFollowersOnly(null), Math.max(0, Date.parse(followersOnly) - Date.now()));
    return () => clearTimeout(timer);
  }, [followersOnly]);
  async function limitToSubs(on: boolean) {
    const result = await send<{ subs_only: boolean }>("PUT", `${path}/subs-only`, { on });
    if (result.ok) setSubsOnly(result.data.subs_only); else setError(result.error);
  }
  // Shield mode (docs/LIVE_STREAMS.md "Shield mode"): 10, 30 or 60 minutes, always ends by itself.
  async function protect(on: boolean, minutes = 10) {
    const result = await send<{ followers_only_until: string | null }>("PUT", `${path}/protect`, { on, minutes });
    if (result.ok) { setFollowersOnly(result.data.followers_only_until); setSpike(false); } else setError(result.error);
  }

  useEffect(() => {
    let closed = false;
    let poll: ReturnType<typeof setInterval> | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const settleIn = (list: Message[]) => { const s = sending.current; if (s && list.some(m => m.id === s.id)) ack(s.id); };
    const load = async () => { const r = await send<Snapshot>("GET", path); if (!closed && r.ok) { merge(r.data.messages, true); settleIn(r.data.messages); setPinned(r.data.pinned); setEmotes(r.data.emotes ?? []); setFollowersOnly(r.data.followers_only_until ?? null); setSubsOnly(!!r.data.subs_only); setAllowSignatures(r.data.allow_signatures ?? true); setGifs(r.data.gifs ?? null); setError(""); } else if (!closed && !r.ok) { setError(r.error); if (squad) merge([], true); } };
    // Polling covers only the time the socket is down; the socket's snapshot takes over again.
    const startPoll = () => {
      if (closed || poll) return;
      setMode("polling");
      void load();
      poll = setInterval(() => { if (!document.hidden) void load(); }, 4000);
    };
    // Drops since the last good snapshot (the backoff), and whether any socket has opened.
    let attempt = 0;
    let everOpened = false;
    const open = () => {
      const ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/chat/ws?${squad ? `squad=${encodeURIComponent(squad)}` : `channel=${encodeURIComponent(username)}`}`);
      socket.current = ws;
      ws.onopen = () => { everOpened = true; clearInterval(poll); poll = undefined; setMode("live"); };
      ws.onmessage = event => {
        if (closed) return;
        const data = JSON.parse(event.data) as Event;
        // A co-stream member's CrowdSync and raid events name their own channel.
        const channel = ("source" in data && data.source ? data.source : username).toLowerCase();
        if (data.type === "snapshot") { attempt = 0; merge(data.messages, true); settleIn(data.messages); setPinned(data.pinned); setEmotes(data.emotes ?? []); setFollowersOnly(data.followers_only_until ?? null); setSubsOnly(!!data.subs_only); setAllowSignatures(data.allow_signatures ?? true); setGifs(data.gifs ?? null); }
        else if (data.type === "protect") setFollowersOnly(data.until);
        else if (data.type === "subs_only") setSubsOnly(data.on);
        else if (data.type === "emotes") {
          if (data.emotes) setEmotes(data.emotes);
          if (typeof data.allow_signatures === "boolean") setAllowSignatures(data.allow_signatures);
          void refreshSignatures();
          // Only a changed outside set reloads it, spread over 3 seconds so viewers don't all ask at once.
          if (data.outside) setTimeout(() => { if (!closed) setEmotesVersion(v => v + 1); }, Math.random() * 3000);
        }
        else if (data.type === "pin") setPinned(data.pinned);
        else if (data.type === "message" || data.type === "ack") {
          merge([data.message]);
          // The phone watch page marks its Chat switch while chat is tucked away (WatchLayout).
          if (data.type === "message") window.dispatchEvent(new Event("sver:chat-message"));
          // The draft is cleared only once the server has the message (unless it was edited since).
          if (data.type === "ack") ack(data.id);
        }
        else if (data.type === "delete") remove(data.id);
        // The player on this page runs the raid countdown (LivePlayer listens for this).
        else if (data.type === "raid" || data.type === "raid_cancelled") window.dispatchEvent(new CustomEvent("sver:raid", { detail: { channel, raid: data.type === "raid" ? data.raid : null } }));
        // Board presses, joystick moves, board changes, polls and counters go to the player's effects and the CrowdSync panels.
        else if (BOARD_EVENTS.has(data.type)) window.dispatchEvent(new CustomEvent("sver:board", { detail: { channel, event: data } }));
        else if (data.type === "system") setNotice(data.text);
        else if (data.type === "error") {
          // A refusal of the send ends it (an edit gets a new id); an id-less one ("too fast") means the
          // frame went unread, so Send works again under the same id.
          if (data.id && sending.current?.id === data.id) { sending.current = null; pending.clear(); }
          else if (!data.id) sending.current = null;
          setError(data.message);
          startWait(data.retry);
        }
      };
      ws.onclose = event => {
        if (socket.current === ws) socket.current = null;
        if (closed) return;
        // 4003: this chat became age-restricted for this viewer. Final; one read shows the server's reason.
        if (event.code === 4003) { void load(); return; }
        // Never gives up. Polls meanwhile if no socket ever opened or the outage outlasts two retries.
        if (!everOpened || attempt >= 2) startPoll(); else setMode("connecting");
        retry = setTimeout(open, reconnectDelay(attempt, event.code));
        attempt += 1;
      };
    };
    open();
    return () => { closed = true; clearInterval(poll); clearTimeout(retry); socket.current?.close(); };
  }, [path, username, squad, merge, remove, ack, pending, startWait, refreshSignatures]);

  // New messages are followed only while the reader is at the bottom (see the list's onScroll);
  // scrolling the list itself never moves the page.
  // Scrolled up: a button says newer messages arrived below instead of pulling the reader down.
  const [behind, setBehind] = useState(false);
  useEffect(() => {
    const el = list.current;
    const newest = messages.at(-1)?.seq ?? 0;
    if (el && follow.current) { el.scrollTop = el.scrollHeight; newestSeen.current = newest; }
    else if (loaded.current && newest > newestSeen.current) setBehind(true);
    if (loaded.current) setLive(true);
  }, [messages]);
  // Back at the bottom: drop the extra scrollback again.
  const atBottom = () => { follow.current = true; setBehind(false); newestSeen.current = messages.at(-1)?.seq ?? 0; setMessages(m => m.length > 100 ? m.slice(-100) : m); };
  const toNewest = () => { const el = list.current; if (el) el.scrollTop = el.scrollHeight; atBottom(); };

  // Act first, review later: no reason now; the moderator gives one in Studio → Chat after the stream
  // (shared co-stream chats still take a stock reason). A ban always confirms.
  async function restrict(kind: "timeout" | "ban", who: string, minutes?: number) {
    if (kind === "ban" && !window.confirm(squad ? `Ban @${who} from this co-stream's shared chat? It ends with the co-stream and doesn't block watching.` : `Ban @${who} from this chat? They also can't watch here while signed in.`)) return false;
    const reason = squad ? { timeout: "Timeout", ban: "Ban" }[kind] : "";
    const result = await send("POST", `${path}/restrictions`, { username: who, kind, seconds: minutes === undefined ? undefined : minutes * 60, reason });
    if (!result.ok) { setError(result.error); return false; }
    if (squad) await loadRole();
    return true;
  }

  /** `quick`: the inline tools — no prompts and a 10-minute timeout; a ban still confirms. */
  async function moderate(action: "delete" | "timeout" | "ban", m: Message, quick = false) {
    const who = m.author.username;
    if (!who) return;
    if (action === "delete") {
      const result = await send("DELETE", `${path}/messages/${m.id}`, {});
      if (result.ok) remove(m.id); else setError(result.error);
      return;
    }
    let minutes: number | undefined;
    if (action === "timeout") {
      minutes = quick ? 10 : Number(window.prompt(`Time out @${who} for how many minutes? (1–20160)`, "10"));
      if (!Number.isInteger(minutes) || minutes < 1 || minutes > 20160) return;
    }
    await restrict(action, who, minutes);
  }

  // Mirrors the server's live check: anyone with a role deletes their own lines; moderators can't touch
  // the owner's, other moderators' or staff's; nobody times out or bans someone protected. Falls back to
  // the role stored with the message when the live field is missing.
  const own = (m: Message) => !!account && m.author.username === account;
  const isProtected = (m: Message) => m.protected ?? !!m.role;
  const canDelete = (m: Message) => !!role && !!m.author.username && (own(m) || role !== "moderator" || !isProtected(m));
  const canSanction = (m: Message) => !!role && !!m.author.username && !own(m) && !isProtected(m);

  /** Owner only: appoint or remove a channel moderator (the same endpoints as Studio → Chat). */
  async function setModerator(name: string, on: boolean) {
    if (!on && !window.confirm(`Remove @${name} as a moderator?`)) return false;
    const base = `/api/channels/${encodeURIComponent(username)}/moderators`;
    const result = on ? await send("POST", base, { username: name }) : await send("DELETE", `${base}/${encodeURIComponent(name)}`);
    if (result.ok) setNotice(on ? `@${name} is now a moderator.` : `@${name} is no longer a moderator.`);
    else setError(result.status === 403 && result.error.includes("Confirm") ? "Confirm your sign-in method again on the Account page, then retry." : result.error);
    return result.ok;
  }

  async function outsideAction(m: Message, action: "hide" | "mute" | "mute-forever") {
    const o = m.outside;
    if (!o) return;
    const result = action === "hide"
      ? await send("POST", `${path}/outside/${encodeURIComponent(m.id)}/hide`)
      : await send("POST", `${path}/outside-mutes`, { platform: o.platform, sender_id: o.sender_id, permanent: action === "mute-forever" });
    if (!result.ok) setError(result.error);
  }

  async function changePin(messageId: string | null, reason = "") {
    const result = await send<{ pinned: Message | null }>("PUT", `${path}/pin`, { message_id: messageId, reason });
    if (result.ok) setPinned(result.data.pinned); else setError(result.error);
  }

  async function submit(event: FormEvent, pinDraft = false) {
    event.preventDefault();
    const body = draft.trim();
    if (!body || busy || wait) return;
    setError("");
    // Owner commands: /raid username starts a raid, /unraid cancels it during the countdown.
    if (/^\/flag$/i.test(body) && !pinDraft) {
      const result = await send("POST", "/api/me/magnet/flag");
      if (result.ok) setDraft(""); else setError(result.error);
      return;
    }
    // /help lists what works in this chat, including the channel's custom commands.
    if (/^\/help$/i.test(body) && !pinDraft) {
      const result = await send<{ commands: { name: string; access: string }[]; bot: string }>("GET", `/api/channels/${encodeURIComponent(username)}/commands`);
      const custom = result.ok && result.data.commands.length ? ` · ${result.data.bot}'s commands: ${result.data.commands.map(c => `!${c.name}${c.access === "everyone" ? "" : ` (${c.access})`}`).join(", ")}` : "";
      setNotice(`Commands: !rally (rally your faction) · counters such as !deaths${role ? ` · moderators: /timeout name [minutes], /ban name, Timeout and Ban from a message's Actions${canPin ? ", !marker, +/- on counters" : ""}` : ""}${role === "owner" ? " · you: /raid name, /unraid, /mod name, /unmod name, /flag" : ""}${custom}`);
      setDraft("");
      return;
    }
    // The streamer posts out: a reply to an outside message, or the platform chosen in "Also post on".
    const platform = outsideReply?.platform ?? (squad ? "" : postOn);
    if (platform && !pinDraft) {
      setBusy(true);
      const result = await send("POST", "/api/me/linked-chat/reply", { platform, body, ...(outsideReply ? { reply_to: outsideReply.id } : {}) });
      setBusy(false);
      if (result.ok) { setDraft(""); setOutsideReply(null); } else { setError(result.error); startWait(result.retry); }
      return;
    }
    const mod = /^\/(mod|unmod)\s+@?([A-Za-z0-9_]{3,25})$/i.exec(body);
    if (mod && role === "owner" && !squad && !pinDraft) {
      if (await setModerator(mod[2], mod[1].toLowerCase() === "mod")) setDraft("");
      return;
    }
    // Moderators (shared co-stream chat too): /timeout name [minutes] (10 by default), /ban name.
    const sanction = /^\/(ban|timeout)\s+@?([A-Za-z0-9_]{3,25})(?:\s+(\d+))?$/i.exec(body);
    if (sanction && role && !pinDraft) {
      const kind = sanction[1].toLowerCase() === "ban" ? "ban" : "timeout";
      const minutes = kind === "timeout" ? Number(sanction[3] ?? 10) : undefined;
      if (minutes !== undefined && (minutes < 1 || minutes > 20160)) { setError("Use /timeout name minutes (1–20160)."); return; }
      if (await restrict(kind, sanction[2], minutes)) { setDraft(""); setNotice(kind === "ban" ? `@${sanction[2]} is banned.` : `@${sanction[2]} is timed out for ${minutes} minutes.`); }
      return;
    }
    const raid = /^\/(raid|unraid)(?:\s+@?([A-Za-z0-9_]{3,25}))?$/i.exec(body);
    if (raid && !pinDraft) {
      if (raid[1].toLowerCase() === "raid" && !raid[2]) { setError("Use /raid username."); return; }
      const result = raid[1].toLowerCase() === "raid" ? await send("POST", "/api/me/raids", { username: raid[2] }) : await send("DELETE", "/api/me/raids");
      if (result.ok) setDraft(""); else setError(result.error);
      return;
    }
    const id = pending.id(JSON.stringify({ body, reply: reply?.id, tribute, highlight: highlight !== null, pin: pinDraft }));
    const command = { id, body, reply_to: reply?.id, ...(tribute ? { tribute } : {}), ...(highlight !== null ? { highlight: true } : {}) };
    // Tributes go over HTTP so a refusal (balance, channel can't earn) shows before anything moves.
    const ws = socket.current;
    if (!pinDraft && !tribute && highlight === null && ws?.readyState === WebSocket.OPEN) {
      if (sending.current?.id === id && sending.current.ws === ws) return;
      // Kept until the ack, so a refusal (slow mode, rate) or a dropped socket doesn't lose the text.
      sending.current = { id, body: draft, ws };
      ws.send(JSON.stringify(command));
      return;
    }
    setBusy(true);
    const result = await send<{ message: Message }>("POST", path, command);
    pending.settle(result);
    if (result.ok) {
      merge([result.data.message]); setDraft(""); setReply(null); setTribute(null);
      if (sending.current?.id === id) sending.current = null;
      if (highlight !== null) { setHighlight(null); setRewardsVersion(v => v + 1); }
      if (pinDraft) await changePin(result.data.message.id);
    } else { setError(result.error); startWait(result.retry); }
    setBusy(false);
  }

  // GIFs go over HTTP so a refusal (the channel's GIF setting, slow mode) shows at once.
  async function sendGif(gif: Gif & { title: string }) {
    if (wait) return;
    setPicking(false); setError(""); setBusy(true);
    const result = await send<{ message: Message }>("POST", path, { id: gifPending.id(JSON.stringify({ gif: gif.slug, reply: reply?.id })), body: "", reply_to: reply?.id, gif });
    gifPending.settle(result);
    if (result.ok) { merge([result.data.message]); setReply(null); } else { setError(result.error); startWait(result.retry); }
    setBusy(false);
  }

  if (variant === "overlay") return <section className="chat chat-overlay" aria-label="Chat">
    <ol className="chat-messages" aria-live="polite">{messages.filter(m => (!m.outside || m.outside.platform === "bot") && clock - (m.arrived ?? clock) < fade * 1000).map(m => <li key={m.id}>
      {m.outside ? <><span className="badge bot-badge">Bot</span> <strong>{m.outside.name}</strong></> : <Identity m={m} host={host} card={false} />}: <MessageBody message={m} account={null} lookup={shown} />
    </li>)}</ol>
  </section>;
  return <section className={variant === "dock" ? "chat panel chat-compact" : "chat panel"} aria-label="Chat">
    {variant !== "dock" && <h2>Chat {mode !== "live" && <span className="muted small">{mode === "polling" ? "(updates every few seconds)" : "(connecting…)"}</span>}
      {variant === "full" && <button type="button" className="small quiet chat-popout" onClick={() => window.open(`/${encodeURIComponent(username)}/chat`, `sver-chat-${username}`, "width=400,height=700")}>Pop out</button>}</h2>}
    {top}
    {pinned && <aside className="chat-pin" aria-label="Pinned message" aria-live="polite">
      <strong>Pinned message</strong><div className="chat-pin-content"><strong>{pinned.author.display_name}: </strong><MessageBody message={pinned} account={account} lookup={shown} /></div>
      {canPin && <button type="button" className="small quiet" onClick={() => changePin(null)}>Unpin</button>}
    </aside>}
    {notice && <p className="chat-system" role="status">{notice}</p>}
    {linkedDown.map(text => <p key={text} className="chat-system" role="status">{text} <Link href="/studio/linked-chat">Linked chat settings</Link></p>)}
    {followersOnly && <p className="chat-system" role="status">Shield mode until {time(followersOnly)}: only followers of at least 10 minutes with accounts 7+ days old can chat, links are off and slow mode is on.{canPin && <> <button type="button" className="small quiet" onClick={() => protect(false)}>End now</button></>}</p>}
    {canPin && !followersOnly && !spike && <p className="chat-system">Shield mode: {[10, 30, 60].map(m => <button key={m} type="button" className="small quiet" onClick={() => protect(true, m)} aria-label={`Shield mode for ${m} minutes`}>{m} min</button>)}</p>}
    {subsOnly ? <p className="chat-system" role="status">Subscriber-only chat.{canPin && <> <button type="button" className="small quiet" onClick={() => limitToSubs(false)}>Open to everyone</button></>}</p>
      : canPin && <p className="chat-system"><button type="button" className="small quiet" onClick={() => limitToSubs(true)}>Subscriber-only chat</button></p>}
    {canPin && spike && !followersOnly && <div className="chat-system" role="alert">A sudden wave of new viewers arrived. <button type="button" className="small" onClick={() => protect(true)}>Shield mode for 10 minutes</button> <button type="button" className="small quiet" onClick={() => setSpike(false)}>Dismiss</button></div>}
    <ol className="chat-messages" ref={list} role="log" aria-live={live ? "polite" : "off"} onScroll={e => { const el = e.currentTarget; const was = follow.current; follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80; if (follow.current && !was) atBottom(); }}>
      {messages.length === 0 && <li className="muted">No messages yet.</li>}
      {messages.map(m => m.outside?.platform === "bot" ? <li key={m.id} className="chat-bot">
        <span className="muted">{time(m.created_at)}</span>{" "}
        <span className="badge bot-badge" title="This channel's bot">Bot</span> <strong>{m.outside.name}</strong>: <MessageBody message={m} account={account} lookup={shown} />
        {canPin && <details className="chat-message-actions"><summary aria-label={`Actions for message from ${m.outside.name}`}>Actions</summary><div className="chat-message-controls">
          <button type="button" className="small quiet" onClick={() => outsideAction(m, "hide")}>Hide here</button>
        </div></details>}
      </li> : m.outside ? <li key={m.id} className="chat-outside">
        <span className="muted">{time(m.created_at)}</span>{" "}
        <span className={`badge platform-badge ${m.outside.platform}`} title={`From ${PLATFORM_NAMES[m.outside.platform] ?? m.outside.platform}`}>{PLATFORM_NAMES[m.outside.platform] ?? m.outside.platform}<span className="sr-only"> message</span></span>{" "}
        {m.outside.url ? <a href={m.outside.url} target="_blank" rel="noopener noreferrer nofollow" title={`${m.outside.name} on ${PLATFORM_NAMES[m.outside.platform] ?? m.outside.platform}`}><strong>{m.outside.name}</strong></a> : <strong>{m.outside.name}</strong>}
        {m.outside.role && <span className="badge">{PLATFORM_NAMES[m.outside.platform] ?? m.outside.platform} {m.outside.role === "moderator" ? "mod" : m.outside.role === "subscriber" ? (m.outside.platform === "youtube" ? "member" : "sub") : "broadcaster"}</span>}: <MessageBody message={m} account={null} lookup={outsideShown} />
        {canPin && <details className="chat-message-actions"><summary aria-label={`Actions for message from ${m.outside.name}`}>Actions</summary><div className="chat-message-controls">
          {role === "owner" && <button type="button" className="small quiet" onClick={() => { setOutsideReply({ id: m.id, platform: m.outside!.platform, name: m.outside!.name }); setReply(null); input.current?.focus(); }}>Reply on {PLATFORM_NAMES[m.outside.platform] ?? m.outside.platform}</button>}
          <button type="button" className="small quiet" onClick={() => outsideAction(m, "hide")}>Hide here</button>
          <button type="button" className="small quiet" onClick={() => outsideAction(m, "mute")}>Mute here for this stream</button>
          <button type="button" className="small quiet" onClick={() => outsideAction(m, "mute-forever")}>Mute here permanently</button>
        </div></details>}
        {account && role !== "owner" && <> <ReportButton target={{ target_type: "outside_message", target_id: m.id }} /></>}
      </li> : <ChatLine key={m.id} m={m} account={account} lookup={shown} host={host} tools={canDelete(m) && <span className="chat-quick-mod">
          <button type="button" className="chat-quick" onClick={() => moderate("delete", m, true)} aria-label={`Delete message from ${m.author.display_name}`} title="Delete"><TrashIcon /></button>
          {canSanction(m) && <>
            <button type="button" className="chat-quick" onClick={() => moderate("timeout", m, true)} aria-label={`Time out ${m.author.display_name} for 10 minutes`} title="Timeout 10 min"><ClockIcon /></button>
            <button type="button" className="chat-quick" onClick={() => moderate("ban", m, true)} aria-label={`Ban ${m.author.display_name}`} title="Ban"><BanIcon /></button>
          </>}
        </span>}>
        <details className="chat-message-actions"><summary aria-label={`Actions for message from ${m.author.display_name}`}>Actions</summary><div className="chat-message-controls">
        {account && <button type="button" className="small quiet" aria-label={`Reply to ${m.author.display_name}`} onClick={() => { setReply({ id: m.id, username: m.author.username, body: Array.from(m.body.replace(/[\r\n]+/g, " ")).slice(0, 80).join("") }); input.current?.focus(); }}>Reply</button>}
        {canPin && <button type="button" className="small quiet" aria-label={`Pin message from ${m.author.display_name}`} onClick={() => changePin(m.id)}>Pin</button>}
        {account && m.author.username && m.author.username !== account && <ReportButton target={{ target_type: "chat_message", target_id: m.id }} />}
        {(!account || !m.author.username || m.author.username === account) && <TakeDownLink target={{ target_type: "chat_message", target_id: m.id }} />}
        {canDelete(m) && <span className="chat-actions">
          <button type="button" className="small quiet" onClick={() => moderate("delete", m)} aria-label={`Delete message from ${m.author.display_name}`}>Delete</button>
          {canSanction(m) && <button type="button" className="small quiet" onClick={() => moderate("timeout", m)} aria-label={`Time out ${m.author.display_name}`}>Timeout</button>}
          {canSanction(m) && <button type="button" className="small quiet" onClick={() => moderate("ban", m)} aria-label={`Ban ${m.author.display_name}`}>Ban</button>}
          {role === "owner" && !squad && m.role !== "owner" && m.role !== "staff" && <button type="button" className="small quiet" onClick={() => setModerator(m.author.username!, !(m.moderator ?? m.role === "moderator"))}>{m.moderator ?? m.role === "moderator" ? "Unmod" : "Mod"}</button>}
        </span>}
        </div></details>
      </ChatLine>)}
    </ol>
    {behind && <button type="button" className="small chat-newest" onClick={toNewest}>New messages below</button>}
    {squad && role && <details><summary>Shared-chat restrictions ({restrictions.length})</summary><ul className="list">{restrictions.map(r => <li key={`${r.user.username}:${r.kind}`}>{r.user.display_name} · {r.kind}{r.until && ` until ${time(r.until)}`}<button className="small quiet" onClick={async () => { const reason = window.prompt("Reason for lifting this restriction")?.trim(); if (!reason || !r.user.username) return; const result = await send("DELETE", `${path}/restrictions/${encodeURIComponent(r.user.username)}/${r.kind}`, { reason }); if (!result.ok) setError(result.error); else await loadRole(); }}>Lift</button></li>)}</ul></details>}
    {account ? <form onSubmit={submit} className="chat-form">
      {reply && <div className="chat-reply-draft"><span>Replying to @{reply.username}: {reply.body}</span><button type="button" className="small quiet" onClick={() => setReply(null)}>Cancel reply</button></div>}
      {outsideReply && <div className="chat-reply-draft"><span>Replying on {PLATFORM_NAMES[outsideReply.platform] ?? outsideReply.platform} to {outsideReply.name}, as your {PLATFORM_NAMES[outsideReply.platform] ?? outsideReply.platform} account</span><button type="button" className="small quiet" onClick={() => setOutsideReply(null)}>Cancel reply</button></div>}
      {!squad && !outsideReply && platforms.length > 0 && <div className="chat-post-on">
        <label htmlFor="chat-post-on">Also post on</label>{" "}
        <select id="chat-post-on" value={postOn} onChange={e => setPostOn(e.target.value)}>
          <option value="">S.V.E.R only</option>
          {platforms.map(p => <option key={p} value={p}>{PLATFORM_NAMES[p] ?? p}</option>)}
        </select>
        {postOn && <span className="muted small"> Posts from your {PLATFORM_NAMES[postOn] ?? postOn} account{postOn === "youtube" ? ", up to 200 characters" : ""}; it shows here when {PLATFORM_NAMES[postOn] ?? postOn} delivers it.</span>}
      </div>}
      {highlight !== null && <div className="chat-draft-note"><span>Your next message is highlighted · {highlight.toLocaleString()} Engagement Valor</span><button type="button" className="small quiet" onClick={() => setHighlight(null)}>Not highlighted</button></div>}
      <label htmlFor="chat-input" className="sr-only">Message</label>
      <textarea ref={input} id="chat-input" value={draft} disabled={busy} maxLength={500} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); } }} />
      <button type="submit" disabled={busy || !!wait}>{tribute ? "Pay tribute" : "Send"}</button>
      {tribute === null ? <button type="button" className="quiet small" disabled={busy} onClick={() => setTribute(10)}>Tribute</button>
        : <span className="tribute-draft"><label htmlFor="tribute-amount">Valor</label> <input id="tribute-amount" type="number" min={10} step={1} value={tribute} onChange={e => setTribute(Math.max(0, Math.floor(Number(e.target.value) || 0)))} /> <button type="button" className="quiet small" onClick={() => setTribute(null)}>No tribute</button> <Link href="/wallet" className="small">Get Valor</Link></span>}
      {gifs && !outsideReply && !postOn && <button type="button" className="quiet small" disabled={busy || !!wait} aria-expanded={picking} onClick={() => setPicking(p => !p)}>GIF</button>}
      {gifs && picking && <GifPicker access={gifs} account={account} onPick={sendGif} onClose={() => setPicking(false)} />}
      {canPin && <button type="button" className="quiet small" disabled={busy || !!wait || !draft.trim()} onClick={event => submit(event, true)}>Send and pin</button>}
      <RetryNotice wait={wait} />
      {error && <p role="alert" className="error">{error}</p>}
    </form> : <p className="chat-signin panel"><Link href={variant === "full" ? "/login" : `/login?${new URLSearchParams({ next: `/${username}/chat${variant === "dock" ? "?dock=1" : ""}` })}`}>Log in</Link> or <Link href="/signup">enlist</Link> to chat.</p>}
    {account && <ChatDock username={username} account={account} emotes={emotes} outside={outside} onOutsideReported={() => setEmotesVersion(v => v + 1)} shared={!!squad} rewardsVersion={rewardsVersion} onHighlight={setHighlight}
      onEmote={code => { setDraft(value => `${value}${value && !/\s$/.test(value) ? " " : ""}${code} `.slice(0, 500)); input.current?.focus(); }} />}
  </section>;
}
