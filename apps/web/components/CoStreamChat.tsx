"use client";
import { useCallback, useEffect, useState, type ComponentProps } from "react";
import { send } from "../lib/client-api";
import { Chat } from "./Chat";
import type { Squad } from "./Squads";

/**
 * A channel's chat that follows its co-stream: the shared room while the channel is in a live MERGED
 * co-stream, otherwise the channel's own room. The open chat says when the room may have changed (a `room`
 * frame, or the shared chat closing for good); a slow re-check while visible, and when the page becomes
 * visible again, covers the rest for tabs, pop-outs, OBS docks and overlays left open for hours.
 * `fade` stays the viewed channel's own overlay setting. `top` shows only in the channel's own room.
 */
export function CoStreamChat({ initial, top, ...chat }: Omit<ComponentProps<typeof Chat>, "squad" | "channel" | "onRoom"> & { initial: Squad | null }) {
  const [squad, setSquad] = useState(initial);
  const { username } = chat;
  const check = useCallback(() => {
    if (!document.hidden) void send<{ squad: Squad | null }>("GET", `/api/channels/${encodeURIComponent(username)}/squad`).then(r => { if (r.ok) setSquad(r.data.squad); });
  }, [username]);
  useEffect(() => {
    const timer = setInterval(check, 60000);
    document.addEventListener("visibilitychange", check);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", check); };
  }, [check]);
  const host = squad?.mode === "MERGED" ? squad.members.find(m => m.host)?.username : undefined;
  // Keyed by room, so switching rooms resets the socket and history.
  return squad && host ? <Chat key={`squad:${squad.id}`} {...chat} username={host} squad={squad.id} channel={username} onRoom={check} /> : <Chat key="channel" {...chat} top={top} onRoom={check} />;
}
