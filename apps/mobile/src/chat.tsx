import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DeviceEventEmitter, FlatList, Keyboard, LayoutAnimation, Platform, Pressable, StyleSheet, Text, TextInput, View, useWindowDimensions, type KeyboardEvent, type ListRenderItemInfo } from "react-native";
import { router } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { BOARD_EVENT, BOARD_TYPES } from "./crowd";
import { send } from "./api";
import { useAuth } from "./auth";
import { factionColor } from "./brand";
import { cookieHeader } from "./cookies";
import { SendIcon } from "./icons";
import { SafetySheet, type SafetyTarget } from "./safety";
import { loadJar, newId } from "./session";
import { SkillsSheet } from "./skills";
import { Attempt, parseTribute } from "./spend";
import { fill, font, hexAlpha, ink, inkDim, line, onAccent, space } from "./theme";
import { isChatMessage, type ChatMessage, type ChatRoom } from "./types";
import { Button, Notice, Panel, Skeleton, type } from "./ui";
import { appOrigin, chatSocketUrl } from "./urls";
import { GetValorSheet } from "./valor";

/** The newest messages kept on screen. */
const CAP = 200;
/** The website's role badges (apps/web/components/Chat.tsx). */
const ROLES: Record<string, string> = { owner: "Broadcaster", moderator: "Moderator", staff: "Staff" };
const LOADING = ["40%", "75%", "55%", "90%", "65%", "80%"] as const;

function merge(current: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const byId = new Map(current.map(message => [message.id, message]));
  for (const message of incoming) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => a.seq - b.seq).slice(-CAP);
}

function openSocket(url: string, headers: Record<string, string>): WebSocket {
  const Socket = WebSocket as unknown as new (url: string, protocols?: string[], options?: { headers?: Record<string, string> }) => WebSocket;
  return new Socket(url, undefined, { headers });
}

/**
 * A channel's chat: dense rows, newest at the bottom, and the composer. It owns its keyboard
 * avoidance, so the player and streamer bar above it never move. Its bottom edge is the window's
 * bottom (Watch's pane and side panel), so the lift is the keyboard's height less the home inset. `onUnread` fires when messages
 * arrive while `active` is false. The socket also carries CrowdSync events (BOARD_EVENT), so keep
 * this mounted while the stream's Play panels are.
 */
export function Chat({ username, active, onUnread, readOnly }: { username: string; active: boolean; onUnread: () => void; readOnly?: boolean }) {
  const insets = useSafeAreaInsets();
  const { account, colors } = useAuth();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [pinned, setPinned] = useState<ChatMessage | null>(null);
  const [followersOnly, setFollowersOnly] = useState<string | null>(null);
  const [subsOnly, setSubsOnly] = useState(false);
  const [error, setError] = useState("");
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);
  // Hidden at once after a block; the server leaves them out of history and the socket from then on.
  const [blocked, setBlocked] = useState<Set<string>>(new Set());
  const [unseen, setUnseen] = useState(0);
  const list = useRef<FlatList<ChatMessage>>(null);
  const away = useRef(false);
  const buffer = useRef<ChatMessage[]>([]);
  const unread = useRef({ active, onUnread });
  useEffect(() => { unread.current = { active, onUnread }; });
  const { height } = useWindowDimensions();
  const [keyTop, setKeyTop] = useState(0);
  useEffect(() => {
    const move = (e: KeyboardEvent | null) => {
      if (e?.duration) LayoutAnimation.configureNext({ duration: e.duration, update: { duration: e.duration, type: LayoutAnimation.Types.keyboard } });
      setKeyTop(e?.endCoordinates.screenY ?? 0);
    };
    const ios = Platform.OS === "ios";
    const subs = [Keyboard.addListener(ios ? "keyboardWillChangeFrame" : "keyboardDidShow", move), Keyboard.addListener(ios ? "keyboardWillHide" : "keyboardDidHide", () => move(null))];
    return () => subs.forEach(sub => sub.remove());
  }, []);
  // A window the system already shrank for the keyboard (Android resize) needs no lift.
  const lift = keyTop > 0 ? Math.max(0, height - keyTop - insets.bottom) : 0;

  const pull = useCallback(async () => {
    const result = await send<Partial<ChatRoom>>("GET", `/api/channels/${encodeURIComponent(username)}/chat`);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    const incoming = Array.isArray(result.data.messages) ? result.data.messages.filter(isChatMessage) : [];
    setMessages(current => merge(current, incoming));
    setPinned(isChatMessage(result.data.pinned) ? result.data.pinned : null);
    setFollowersOnly(result.data.followers_only_until ?? null);
    setSubsOnly(!!result.data.subs_only);
    setLoaded(true);
    setError("");
  }, [username]);

  // Socket messages land in a buffer drawn every 150 ms, so a busy chat renders a few times a second, not per message.
  useEffect(() => {
    const timer = setInterval(() => {
      const batch = buffer.current;
      if (!batch.length) return;
      buffer.current = [];
      setMessages(current => merge(current, batch));
      if (!unread.current.active) unread.current.onUnread();
      if (away.current) setUnseen(n => n + batch.length);
    }, 150);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    let closed = false;
    let socket: WebSocket | null = null;
    let poll: ReturnType<typeof setInterval> | null = null;
    const stopPoll = () => {
      if (poll) clearInterval(poll);
      poll = null;
    };
    const startPoll = () => {
      if (poll || closed) return;
      poll = setInterval(() => { void pull(); }, 5000);
    };
    // History arrives after the request. The rule treats the call itself as a render update.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void pull();
    startPoll();
    void (async () => {
      const jar = await loadJar();
      if (closed) return;
      const headers: Record<string, string> = { Origin: appOrigin };
      const cookie = cookieHeader(jar);
      if (cookie) headers.Cookie = cookie;
      try {
        socket = openSocket(chatSocketUrl(username), headers);
      } catch {
        return;
      }
      socket.onopen = () => stopPoll();
      socket.onclose = () => startPoll();
      socket.onerror = () => socket?.close();
      socket.onmessage = event => {
        if (typeof event.data !== "string") return;
        let payload: unknown;
        try { payload = JSON.parse(event.data); } catch { return; }
        if (!payload || typeof payload !== "object") return;
        const record = payload as Partial<ChatRoom> & { type?: string; message?: unknown; id?: unknown; until?: unknown; on?: unknown };
        if (record.type === "snapshot" && Array.isArray(record.messages)) {
          buffer.current = [];
          setMessages(record.messages.filter(isChatMessage).slice(-CAP));
          setPinned(isChatMessage(record.pinned) ? record.pinned : null);
          setFollowersOnly(record.followers_only_until ?? null);
          setSubsOnly(!!record.subs_only);
          setLoaded(true);
        } else if (record.type === "message" && isChatMessage(record.message)) {
          buffer.current.push(record.message);
        } else if (record.type === "delete" && typeof record.id === "string") {
          const id = record.id;
          buffer.current = buffer.current.filter(message => message.id !== id);
          setMessages(current => current.filter(message => message.id !== id));
        } else if (record.type === "pin") {
          setPinned(isChatMessage(record.pinned) ? record.pinned : null);
        } else if (record.type === "protect") {
          setFollowersOnly(typeof record.until === "string" ? record.until : null);
        } else if (record.type === "subs_only") {
          setSubsOnly(record.on === true);
        } else if (record.type && BOARD_TYPES.has(record.type)) {
          // Board presses, polls, counters, rally and Surge go to the CrowdSync panels and player effects (src/crowd.tsx).
          DeviceEventEmitter.emit(BOARD_EVENT, { channel: username.toLowerCase(), event: payload });
        }
      };
    })();
    return () => {
      closed = true;
      stopPoll();
      socket?.close();
    };
  }, [pull, username, account?.id]);

  // Newest first for the inverted list.
  const shown = useMemo(() => messages.filter(message => !message.author.username || !blocked.has(message.author.username)).reverse(), [messages, blocked]);
  const report = useCallback((message: ChatMessage) => setReporting({ type: "chat_message", id: message.id, label: "message", username: message.author.username, displayName: message.author.display_name }), []);
  const renderItem = useCallback(({ item }: ListRenderItemInfo<ChatMessage>) => <Row message={item} onReport={report} />, [report]);
  const border = line(colors.accent);

  return (
    <View style={[styles.fill, { paddingBottom: lift }]}>
      {pinned ? (
        <Panel style={styles.pinned}>
          <Text style={type.label}>Pinned</Text>
          <Text style={styles.text} numberOfLines={2}><Name message={pinned} />{pinned.body ? `: ${pinned.body}` : ""}</Text>
        </Panel>
      ) : null}
      {error ? <Notice onRetry={() => { void pull(); }}>{error}</Notice> : null}
      <View style={styles.fill}>
        {!loaded ? (
          error ? null : <View style={styles.loading} accessible accessibilityLabel="Loading">{LOADING.map(w => <Skeleton key={w} w={w} h={14} />)}</View>
        ) : shown.length === 0 ? (
          <Text style={[type.small, styles.empty]}>No messages yet. Say hello.</Text>
        ) : (
          <FlatList
            ref={list}
            inverted
            data={shown}
            keyExtractor={message => message.id}
            renderItem={renderItem}
            maintainVisibleContentPosition={{ minIndexForVisible: 0, autoscrollToTopThreshold: 48 }}
            keyboardDismissMode="on-drag"
            keyboardShouldPersistTaps="handled"
            scrollEventThrottle={100}
            onScroll={event => {
              const far = event.nativeEvent.contentOffset.y > 48;
              if (far === away.current) return;
              away.current = far;
              if (!far) setUnseen(0);
            }}
          />
        )}
        {unseen > 0 ? (
          <View pointerEvents="box-none" style={styles.pillSpot}>
            <Pressable accessibilityRole="button" hitSlop={6} onPress={() => { list.current?.scrollToOffset({ offset: 0, animated: true }); setUnseen(0); }}
              style={({ pressed }) => [styles.pill, { backgroundColor: colors.accent }, pressed && styles.pressed]}>
              <Text style={styles.pillText}>{unseen === 1 ? "1 new message" : `${unseen} new messages`}</Text>
            </Pressable>
          </View>
        ) : null}
      </View>
      <SafetySheet target={reporting} onClose={() => setReporting(null)} onBlocked={name => setBlocked(current => new Set(current).add(name))} />
      {readOnly ? null : (
        <View style={[styles.composer, { borderTopColor: border, paddingBottom: insets.bottom + space.s }]}>
          {followersOnly ? <Text style={type.small}>Followers-only chat</Text> : null}
          {subsOnly ? <Text style={type.small}>Subscriber-only chat</Text> : null}
          {account ? (
            <Composer username={username} border={border} onSent={failure => { setError(failure); if (!failure) void pull(); }} />
          ) : (
            <Button tone="quiet" label="Log in to chat" onPress={() => router.push("/sign-in")} />
          )}
        </View>
      )}
    </View>
  );
}

/**
 * The input row, then Tribute and Skills. Typing re-renders only this, not the message list. `onSent`
 * gets "" or the error. A tribute is paid with the message over HTTP, so a refusal shows before any
 * Valor moves; "Pay tribute" is the confirmation, as on the website.
 */
function Composer({ username, border, onSent }: { username: string; border: string; onSent: (error: string) => void }) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  // The tribute field's text while Tribute is on; null when off.
  const [tribute, setTribute] = useState<string | null>(null);
  const [skills, setSkills] = useState(false);
  const [getValor, setGetValor] = useState(false);
  const [attempt] = useState(() => new Attempt(newId));
  const ready = body.trim().length > 0;
  async function submit() {
    const text = body.trim();
    if (!text || busy) return;
    const valor = tribute === null ? null : parseTribute(tribute);
    if (tribute !== null && valor === null) { onSent("A tribute is at least 10 Valor."); return; }
    setBusy(true);
    // A lost answer keeps the id, so trying again can't pay twice. Only the amount starts a new tribute.
    const id = attempt.take(valor ? `tribute:${valor}` : `message:${text}`);
    const result = await send("POST", `/api/channels/${encodeURIComponent(username)}/chat`, { id, body: text, ...(valor ? { tribute: valor } : {}) });
    // A retry whose first try went through: the server already has this message, charged once.
    const sent = result.ok || (attempt.reused && result.status === 409);
    attempt.settle(result);
    setBusy(false);
    if (sent) { setBody(""); setTribute(null); }
    onSent(sent ? "" : result.ok ? "" : result.error);
  }
  return (
    <View style={styles.gapS}>
      <View style={styles.inputRow}>
        <TextInput
          accessibilityLabel="Message"
          placeholder="Send a message"
          placeholderTextColor={inkDim}
          value={body}
          onChangeText={setBody}
          maxLength={500}
          returnKeyType="send"
          submitBehavior="submit"
          onSubmitEditing={() => { void submit(); }}
          style={[styles.input, { borderColor: border }]}
        />
        <Button compact label={tribute === null ? "Send" : "Pay tribute"} icon={c => <SendIcon color={c} />} tone={ready ? "primary" : "quiet"} disabled={busy || !ready} onPress={() => { void submit(); }} />
      </View>
      {tribute === null ? (
        <View style={styles.tools}>
          <Button compact tone="quiet" label="Tribute" disabled={busy} onPress={() => setTribute("10")} />
          <Button compact tone="quiet" label="Skills" onPress={() => setSkills(true)} />
        </View>
      ) : (
        <View style={styles.tools}>
          <Text style={type.label}>Valor</Text>
          <TextInput accessibilityLabel="Tribute in Valor" value={tribute} onChangeText={t => setTribute(t.replace(/[^0-9]/g, ""))} keyboardType="number-pad" maxLength={7}
            style={[styles.input, styles.amount, { borderColor: border }]} />
          <Button compact tone="quiet" label="No tribute" onPress={() => setTribute(null)} />
          <Button compact tone="quiet" label="Get Valor" onPress={() => setGetValor(true)} />
        </View>
      )}
      <SkillsSheet username={username} visible={skills} onClose={() => setSkills(false)} />
      <GetValorSheet visible={getValor} onClose={() => setGetValor(false)} />
    </View>
  );
}

function Name({ message }: { message: ChatMessage }) {
  return <Text style={[styles.name, { color: factionColor(message.author.faction) }]}>{message.author.display_name}</Text>;
}

/** One dense row: role, the name in the author's faction color, then the message. Press and hold to report or block. */
const Row = memo(function Row({ message, onReport }: { message: ChatMessage; onReport: (message: ChatMessage) => void }) {
  const { colors } = useAuth();
  const role = message.role ? ROLES[message.role] : undefined;
  const press = () => onReport(message);
  // The website's chat-tribute and chat-highlight rows.
  const marked = message.tribute ? { borderLeftWidth: 3, borderLeftColor: colors.accent, backgroundColor: hexAlpha(colors.accent, 0.1) }
    : message.highlighted ? { backgroundColor: hexAlpha(colors.accentLight, 0.16) } : null;
  return (
    <Pressable onLongPress={press} accessibilityActions={[{ name: "longpress", label: "Report or block" }]} onAccessibilityAction={press} style={[styles.row, marked]}>
      <Text style={styles.text}>
        {message.tribute ? <Text style={[styles.role, { color: colors.accentLight }]}>{message.tribute.toLocaleString()} Valor </Text> : null}
        {role ? <Text style={styles.role}>{role} </Text> : null}
        <Name message={message} />
        {/* A Skill played from the app or the website. */}
        {message.skill ? <Text style={styles.dim}> played {message.skill}</Text> : null}
        {message.body ? `: ${message.body}` : ""}
      </Text>
    </Pressable>
  );
});

const styles = StyleSheet.create({
  fill: { flex: 1 },
  pinned: { padding: space.m, marginHorizontal: space.m, marginTop: space.s, gap: space.xs },
  loading: { padding: space.m, gap: space.m },
  empty: { textAlign: "center", paddingTop: space.xl },
  row: { paddingVertical: space.xs, paddingHorizontal: space.m },
  text: { fontFamily: font.body, fontSize: 14, lineHeight: 20, color: ink },
  role: { fontFamily: font.label, fontSize: 11, letterSpacing: 1, textTransform: "uppercase", color: inkDim },
  name: { fontFamily: font.bodySemi },
  dim: { color: inkDim },
  pillSpot: { position: "absolute", left: 0, right: 0, bottom: space.s, alignItems: "center" },
  pill: { minHeight: 32, justifyContent: "center", paddingHorizontal: space.m },
  pillText: { fontFamily: font.label, fontSize: 12, letterSpacing: 1.4, textTransform: "uppercase", color: onAccent },
  pressed: { opacity: 0.85 },
  composer: { borderTopWidth: 1, paddingTop: space.s, paddingHorizontal: space.m, gap: space.s },
  inputRow: { flexDirection: "row", gap: space.s },
  gapS: { gap: space.s },
  tools: { flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: space.s },
  amount: { flex: 0, width: 96 },
  input: { flex: 1, minHeight: 44, borderWidth: 1, color: ink, backgroundColor: fill, paddingHorizontal: space.m, fontFamily: font.body, fontSize: 15 },
});
