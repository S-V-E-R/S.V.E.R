import { useCallback, useEffect, useState } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";
import { send } from "./api";
import { useAuth } from "./auth";
import { factionColor, factionLabel } from "./brand";
import { cookieHeader } from "./cookies";
import { SafetySheet, type SafetyTarget } from "./safety";
import { loadJar, newId } from "./session";
import { isChatMessage, type ChatMessage } from "./types";
import { Button, Field, Frame, Notice, ink, inkDim, inkMuted } from "./ui";
import { appOrigin, chatSocketUrl } from "./urls";

function merge(current: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const byId = new Map(current.map(message => [message.id, message]));
  for (const message of incoming) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => a.seq - b.seq);
}

function openSocket(url: string, headers: Record<string, string>): WebSocket {
  const Socket = WebSocket as unknown as new (url: string, protocols?: string[], options?: { headers?: Record<string, string> }) => WebSocket;
  return new Socket(url, undefined, { headers });
}

export function Chat({ username }: { username: string }) {
  const { account } = useAuth();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [body, setBody] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [reporting, setReporting] = useState<SafetyTarget | null>(null);
  // Hidden at once after a block; the server leaves them out of history and the socket from then on.
  const [blocked, setBlocked] = useState<Set<string>>(new Set());

  const pull = useCallback(async () => {
    const result = await send<{ messages?: unknown }>("GET", `/api/channels/${encodeURIComponent(username)}/chat`);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    const list = Array.isArray(result.data.messages) ? result.data.messages.filter(isChatMessage) : [];
    setMessages(current => merge(current, list));
    setError("");
  }, [username]);

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
        const record = payload as { type?: string; messages?: unknown; message?: unknown; id?: unknown };
        if (record.type === "snapshot" && Array.isArray(record.messages)) {
          setMessages(record.messages.filter(isChatMessage));
        } else if (record.type === "message" && isChatMessage(record.message)) {
          setMessages(current => merge(current, [record.message as ChatMessage]));
        } else if (record.type === "delete" && typeof record.id === "string") {
          setMessages(current => current.filter(message => message.id !== record.id));
        }
      };
    })();
    return () => {
      closed = true;
      stopPoll();
      socket?.close();
    };
  }, [pull, username, account?.id]);

  async function submit() {
    const text = body.trim();
    if (!text || busy) return;
    if (text.length > 500) {
      setError("Messages are 1–500 characters.");
      return;
    }
    if ((text.match(/\n/g) ?? []).length > 4) {
      setError("Use at most four line breaks.");
      return;
    }
    setBusy(true);
    const result = await send("POST", `/api/channels/${encodeURIComponent(username)}/chat`, { id: newId(), body: text });
    setBusy(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setBody("");
    setError("");
    void pull();
  }

  return (
    <Frame>
      <Text style={styles.heading}>Chat</Text>
      {error ? <Notice>{error}</Notice> : null}
      <View style={styles.list}>
        {messages.length === 0 ? <Text style={styles.empty}>No messages yet.</Text> : messages
          .filter(message => !message.author.username || !blocked.has(message.author.username))
          .map(message => <Message key={message.id} message={message} onReport={() => setReporting({ type: "chat_message", id: message.id, label: "message", username: message.author.username, displayName: message.author.display_name })} />)}
      </View>
      {messages.length > 0 ? <Text style={styles.hint}>Press and hold a message to report it or block its author.</Text> : null}
      <SafetySheet target={reporting} onClose={() => setReporting(null)} onBlocked={name => setBlocked(current => new Set(current).add(name))} />
      {account ? (
        <View style={styles.composer}>
          <Field label="Message" value={body} onChangeText={setBody} multiline onSubmitEditing={() => { void submit(); }} />
          <Button label={busy ? "Sending…" : "Send"} onPress={() => { void submit(); }} disabled={busy || body.trim().length === 0} />
        </View>
      ) : (
        <Text style={styles.empty}>Log in to chat.</Text>
      )}
    </Frame>
  );
}

function Message({ message, onReport }: { message: ChatMessage; onReport: () => void }) {
  const color = factionColor(message.author.faction);
  const side = factionLabel(message.author.faction);
  const role = message.role === "owner" ? "Broadcaster" : message.role === "moderator" ? "Moderator" : message.role === "staff" ? "Staff" : null;
  return (
    <Pressable style={styles.message} onLongPress={onReport} accessibilityRole="button" accessibilityHint="Press and hold to report or block"
      accessibilityActions={[{ name: "longpress", label: "Report or block" }]} onAccessibilityAction={onReport}>
      <Text style={styles.meta}>
        <Text style={{ color }}>{message.author.display_name}</Text>
        {side ? <Text style={styles.dim}> · {side}</Text> : null}
        {role ? <Text style={styles.dim}> · {role}</Text> : null}
      </Text>
      {message.body ? <Text style={styles.body}>{message.body}</Text> : null}
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
  composer: { gap: 12, marginTop: 8 },
  hint: { color: inkDim, fontFamily: "Barlow_400Regular", fontSize: 13, marginTop: 4 },
});
