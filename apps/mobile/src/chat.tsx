import { useCallback, useEffect, useRef, useState } from "react";
import { AccessibilityInfo, AppState, DeviceEventEmitter, Pressable, StyleSheet, Text, View } from "react-native";
import { BOARD_EVENT, BOARD_TYPES } from "./crowd";
import { send } from "./api";
import { useAuth } from "./auth";
import { factionColor, factionLabel } from "./brand";
import { CREATOR_TIERS, PLATFORM_NAMES, SUPPORTER_TIERS, mentions, merge, outsideRole, retryDelay, subBadge } from "./chat-rules";
import { cookieHeader } from "./cookies";
import { SafetySheet, type SafetyTarget } from "./safety";
import { loadJar, newId } from "./session";
import { isChatMessage, type ChatMessage } from "./types";
import { Button, Field, Frame, Notice, hexAlpha, ink, inkDim, inkMuted } from "./ui";
import { appOrigin, chatSocketUrl } from "./urls";

/** What a chat read and the socket snapshot carry besides the messages (chat.rs `read`). */
type Room = { messages?: unknown; pinned?: unknown; followers_only_until?: unknown; subs_only?: unknown };

const time = (iso: string) => new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

function openSocket(url: string, headers: Record<string, string>): WebSocket {
  const Socket = WebSocket as unknown as new (url: string, protocols?: string[], options?: { headers?: Record<string, string> }) => WebSocket;
  return new Socket(url, undefined, { headers });
}

/**
 * A channel's chat, or a merged co-stream's shared chat when `squad` (and its `host`) is set. Live over
 * the socket, which reconnects with backoff; polls only while it's down and the app is in front.
 * `onRoomGone` runs on a 409, so the watch screen can re-check the co-stream and switch rooms.
 */
export function Chat({ username, squad, host, onRoomGone }: { username: string; squad?: string | null; host?: string | null; onRoomGone?: () => unknown }) {
  const { account, colors } = useAuth();
  const room = squad && host ? host : username;
  const path = squad ? `/api/squads/${encodeURIComponent(squad)}/chat` : `/api/channels/${encodeURIComponent(username)}/chat`;
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [pinned, setPinned] = useState<ChatMessage | null>(null);
  const [followersOnly, setFollowersOnly] = useState<string | null>(null);
  const [subsOnly, setSubsOnly] = useState(false);
  const [body, setBody] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [retryAt, setRetryAt] = useState(0);
  const [now, setNow] = useState(0);
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);
  // Hidden at once after a block; the server leaves them out of history and the socket from then on.
  const [blocked, setBlocked] = useState<Set<string>>(new Set());
  // The id of a send that may have reached the server; resending the same text reuses it, so a retry never posts twice.
  const pending = useRef<{ id: string; text: string } | null>(null);

  const apply = useCallback((data: Room) => {
    setMessages(Array.isArray(data.messages) ? data.messages.filter(isChatMessage).slice(-100) : []);
    setPinned(isChatMessage(data.pinned) ? data.pinned : null);
    setFollowersOnly(typeof data.followers_only_until === "string" ? data.followers_only_until : null);
    setSubsOnly(data.subs_only === true);
  }, []);

  const pull = useCallback(async () => {
    const result = await send<Room>("GET", path);
    if (!result.ok) {
      setError(result.error);
      if (result.status === 409) onRoomGone?.();
      return;
    }
    // Replace, not merge: lines deleted or moderated since the last read disappear.
    apply(result.data);
    setError("");
  }, [path, apply, onRoomGone]);

  useEffect(() => {
    let closed = false;
    // Close code 4003: the chat is now mature-gated for this account. Final, no reconnect (chat.rs).
    let final = false;
    let connecting = false;
    let socket: WebSocket | null = null;
    let poll: ReturnType<typeof setInterval> | null = null;
    let retry: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const active = () => AppState.currentState === "active";
    const stopPoll = () => {
      if (poll) clearInterval(poll);
      poll = null;
    };
    const startPoll = () => {
      if (poll || closed) return;
      poll = setInterval(() => { if (active()) void pull(); }, 5000);
    };
    const schedule = (soon: boolean) => {
      if (closed || final || retry || !active()) return;
      retry = setTimeout(() => { retry = null; void open(); }, soon ? 1000 * Math.random() : retryDelay(attempt++));
    };
    const open = async () => {
      if (closed || final || socket || connecting || !active()) return;
      connecting = true;
      const jar = await loadJar();
      connecting = false;
      if (closed || socket) return;
      const headers: Record<string, string> = { Origin: appOrigin };
      const cookie = cookieHeader(jar);
      if (cookie) headers.Cookie = cookie;
      let ws: WebSocket;
      try {
        ws = openSocket(chatSocketUrl(username, squad), headers);
      } catch {
        schedule(false);
        return;
      }
      socket = ws;
      ws.onopen = () => stopPoll();
      ws.onclose = event => {
        if (socket === ws) socket = null;
        if (closed) return;
        if (event.code === 4003) {
          final = true;
          stopPoll();
          void pull();
          return;
        }
        if (!active()) return;
        startPoll();
        // 4000 is the server asking for a resync: come back at once, with jitter only.
        schedule(event.code === 4000);
      };
      ws.onerror = () => ws.close();
      ws.onmessage = event => {
        if (typeof event.data !== "string") return;
        let payload: unknown;
        try { payload = JSON.parse(event.data); } catch { return; }
        if (!payload || typeof payload !== "object") return;
        const record = payload as Room & { type?: string; message?: unknown; id?: unknown; until?: unknown; on?: unknown; source?: unknown };
        if (record.type === "snapshot") {
          attempt = 0;
          apply(record);
        } else if (record.type === "message" && isChatMessage(record.message)) {
          setMessages(current => merge(current, [record.message as ChatMessage]));
        } else if (record.type === "delete" && typeof record.id === "string") {
          const id = record.id;
          setMessages(current => current.filter(message => message.id !== id).map(message => message.reply?.id === id ? { ...message, reply: { id, username: null, body: null } } : message));
        } else if (record.type === "pin") {
          setPinned(isChatMessage(record.pinned) ? record.pinned : null);
        } else if (record.type === "protect") {
          setFollowersOnly(typeof record.until === "string" ? record.until : null);
        } else if (record.type === "subs_only") {
          setSubsOnly(record.on === true);
        } else if (record.type && BOARD_TYPES.has(record.type)) {
          // Board presses, polls, counters, rally and Surge go to the CrowdSync panels and player effects (src/crowd.tsx).
          // A shared chat forwards each member's events with `source`, so each stays with its own channel.
          const channel = typeof record.source === "string" ? record.source : room;
          DeviceEventEmitter.emit(BOARD_EVENT, { channel: channel.toLowerCase(), event: payload });
        }
      };
    };
    const state = AppState.addEventListener("change", next => {
      if (next === "active") {
        attempt = 0;
        void pull();
        void open();
      } else if (next === "background") {
        if (retry) clearTimeout(retry);
        retry = null;
        stopPoll();
        socket?.close();
      }
    });
    // History arrives after the request. The rule treats the call itself as a render update.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void pull();
    startPoll();
    void open();
    return () => {
      closed = true;
      stopPoll();
      if (retry) clearTimeout(retry);
      state.remove();
      socket?.close();
    };
  }, [pull, apply, username, squad, room, account?.id]);

  useEffect(() => {
    const left = retryAt - Date.now();
    if (left <= 0) return;
    // Announced once; the visible countdown below ticks without speaking every second.
    AccessibilityInfo.announceForAccessibility(`You can send again in ${Math.ceil(left / 1000)} seconds.`);
    const timer = setInterval(() => {
      const t = Date.now();
      setNow(t);
      if (t >= retryAt) clearInterval(timer);
    }, 1000);
    return () => clearInterval(timer);
  }, [retryAt]);
  const wait = Math.max(0, Math.ceil((retryAt - now) / 1000));

  async function submit() {
    const text = body.trim();
    if (!text || busy || wait > 0) return;
    if (text.length > 500) {
      setError("Messages are 1–500 characters.");
      return;
    }
    if ((text.match(/\n/g) ?? []).length > 4) {
      setError("Use at most four line breaks.");
      return;
    }
    const id = pending.current?.text === text ? pending.current.id : newId();
    pending.current = { id, text };
    setBusy(true);
    const result = await send<{ message?: unknown }>("POST", path, { id, body: text });
    setBusy(false);
    if (!result.ok) {
      // A refusal (4xx) is final, so an edited or later send gets a new id. A network failure or
      // server error keeps it: the message may have been stored, and the server acks a repeated id.
      if (result.status >= 400 && result.status < 500) pending.current = null;
      setError(result.error);
      if (result.retry) {
        const t = Date.now();
        setNow(t);
        setRetryAt(t + result.retry * 1000);
      }
      if (result.status === 409) onRoomGone?.();
      return;
    }
    pending.current = null;
    setBody("");
    setError("");
    const sent = result.data.message;
    if (isChatMessage(sent)) setMessages(current => merge(current, [sent]));
    else void pull();
  }

  const highlight = hexAlpha(colors.accent, 0.14);
  const report = (message: ChatMessage): SafetyTarget | null => {
    // The channel's bot isn't reportable (linked_chat.rs report_target); other platforms' lines are.
    if (message.outside?.platform === "bot") return null;
    if (message.outside) return { type: "outside_message", id: message.id, label: "message", username: null, displayName: message.outside.name };
    return { type: "chat_message", id: message.id, label: "message", username: message.author.username, displayName: message.author.display_name };
  };
  return (
    <Frame>
      <Text style={styles.heading}>Chat</Text>
      {error ? <Notice>{error}</Notice> : null}
      {pinned ? (
        <View style={[styles.pin, { borderColor: hexAlpha(colors.accent, 0.32) }]} accessibilityLabel={`Pinned message from ${pinned.author.display_name}: ${pinned.body ?? ""}`}>
          <Text style={styles.dim}>Pinned message</Text>
          <Text style={styles.body}><Text style={styles.meta}>{pinned.author.display_name}: </Text>{pinned.body}</Text>
        </View>
      ) : null}
      <View style={styles.list}>
        {messages.length === 0 ? <Text style={styles.empty}>No messages yet.</Text> : messages
          .filter(message => !message.author.username || !blocked.has(message.author.username))
          .map(message => {
            const target = report(message);
            const mine = mentions(message, account?.username);
            return <Message key={message.id} message={message} host={squad ? host ?? null : null} mentioned={mine} highlight={message.tribute || message.highlighted || mine ? highlight : undefined} onReport={target ? () => setReporting(target) : undefined} />;
          })}
      </View>
      {messages.length > 0 ? <Text style={styles.hint}>Press and hold a message to report it. On a S.V.E.R account’s message, you can also block its author.</Text> : null}
      <SafetySheet target={reporting} onClose={() => setReporting(null)} onBlocked={name => setBlocked(current => new Set(current).add(name))} />
      {followersOnly ? <Text style={styles.hint}>Shield mode until {time(followersOnly)}: only followers of at least 10 minutes with accounts 7+ days old can chat, links are off and slow mode is on.</Text> : null}
      {subsOnly ? <Text style={styles.hint}>Subscriber-only chat.</Text> : null}
      {account ? (
        <View style={styles.composer}>
          <Field label="Message" value={body} onChangeText={setBody} multiline onSubmitEditing={() => { void submit(); }} />
          {wait > 0 ? <Text style={styles.hint}>You can send again in {wait}s.</Text> : null}
          <Button label={busy ? "Sending…" : "Send"} onPress={() => { void submit(); }} disabled={busy || wait > 0 || body.trim().length === 0} />
        </View>
      ) : (
        <Text style={styles.empty}>Log in to chat.</Text>
      )}
    </Frame>
  );
}

function Message({ message, host, mentioned, highlight, onReport }: { message: ChatMessage; host: string | null; mentioned: boolean; highlight?: string; onReport?: () => void }) {
  const outside = message.outside;
  const platform = outside ? PLATFORM_NAMES[outside.platform] ?? outside.platform : null;
  const color = outside ? ink : factionColor(message.author.faction);
  const side = outside ? null : factionLabel(message.author.faction);
  // A co-stream's other members are owners of their own channels: "Co-streamer" in the shared chat, as on the website.
  const role = outside ? null : message.role === "owner" ? (host && message.author.username?.toLowerCase() !== host.toLowerCase() ? "Co-streamer" : "Broadcaster") : message.role === "moderator" ? "Moderator" : message.role === "staff" ? "Staff" : null;
  const supporter = message.supporter;
  const content = (
    <>
      {message.reply ? (
        <Text style={styles.quote} numberOfLines={2}>{message.reply.body === null ? "Message deleted" : `${message.reply.username ? `@${message.reply.username}: ` : ""}${message.reply.body}`}</Text>
      ) : null}
      <Text style={styles.meta}>
        {message.tribute ? <Text style={styles.dim}>{message.tribute.toLocaleString()} Valor · </Text> : null}
        {outside?.platform === "bot" ? <Text style={styles.dim} accessibilityLabel="This channel's bot">Bot </Text>
          : platform ? <Text style={styles.dim} accessibilityLabel={`From ${platform}`}>{platform} </Text> : null}
        {!outside && message.author.guild ? <Text style={styles.dim} accessibilityLabel={`Guild ${message.author.guild.name}`}>[{message.author.guild.tag}] </Text> : null}
        <Text style={{ color }}>{outside ? outside.name : message.author.display_name}</Text>
        {outside ? (outsideRole(outside) ? <Text style={styles.dim}> · {outsideRole(outside)}</Text> : null) : (
          <>
            {side ? <Text style={styles.dim}> · {side}</Text> : null}
            {message.creator_tier && CREATOR_TIERS[message.creator_tier] ? <Text style={styles.dim}> · {CREATOR_TIERS[message.creator_tier]}</Text> : null}
            {supporter ? <Text style={styles.dim}> · {SUPPORTER_TIERS[supporter.tier] ?? supporter.tier} supporter</Text> : null}
            {supporter?.founder ? <Text style={styles.dim}> · Founder #{String(supporter.founder).padStart(3, "0")}</Text> : null}
            {message.sub ? <Text style={styles.dim} accessibilityLabel={`Tier ${message.sub.tier} subscriber${message.sub.channel ? ` to ${message.sub.channel}` : ""}, ${subBadge(message.sub.months)}`}> · Sub {subBadge(message.sub.months)}</Text> : null}
            {message.origin ? <Text style={styles.dim}> · MAGNet</Text> : null}
            {role ? <Text style={styles.dim}> · {role}</Text> : null}
          </>
        )}
        {/* Skills are bought and sent on the website; the app only shows them (docs/MOBILE.md). */}
        {message.skill ? <Text style={styles.dim}> · ✨ {message.skill}</Text> : null}
      </Text>
      {message.body ? <Text style={styles.body}>{message.body}</Text> : null}
    </>
  );
  const style = [styles.message, highlight ? { backgroundColor: highlight, paddingHorizontal: 8 } : null];
  const label = mentioned ? "Mentions you" : undefined;
  if (!onReport) return <View style={style} accessibilityHint={label}>{content}</View>;
  const blockable = !outside && !!message.author.username;
  return (
    <Pressable style={style} onLongPress={onReport} accessibilityRole="button" accessibilityHint={[label, blockable ? "Press and hold to report or block" : "Press and hold to report"].filter(Boolean).join(". ")}
      accessibilityActions={[{ name: "longpress", label: blockable ? "Report or block" : "Report" }]} onAccessibilityAction={onReport}>
      {content}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  heading: { fontFamily: "Cinzel_800ExtraBold", color: ink, fontSize: 22, marginBottom: 12 },
  list: { gap: 0 },
  empty: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 15, paddingVertical: 8 },
  message: { paddingVertical: 8, gap: 2 },
  meta: { fontFamily: "Barlow_600SemiBold", fontSize: 14 },
  dim: { color: inkDim, fontFamily: "Barlow_400Regular" },
  body: { color: ink, fontFamily: "Barlow_400Regular", fontSize: 15, lineHeight: 22 },
  quote: { color: inkMuted, fontFamily: "Barlow_400Regular", fontSize: 13, borderLeftWidth: 2, borderLeftColor: inkDim, paddingLeft: 8 },
  pin: { borderWidth: 1, padding: 8, marginBottom: 8, gap: 2 },
  composer: { gap: 12, marginTop: 8 },
  hint: { color: inkDim, fontFamily: "Barlow_400Regular", fontSize: 13, marginTop: 4 },
});
