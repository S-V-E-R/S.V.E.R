"use client";
import { useEffect, useState, type ComponentProps } from "react";
import { send } from "../lib/client-api";
import { Chat } from "./Chat";
import type { Squad } from "./Squads";

/**
 * A channel's chat that follows its co-stream: the shared room while the channel is in a live MERGED
 * co-stream, otherwise the channel's own room. Watch tabs, pop-outs, OBS docks and overlays stay open
 * for hours, so it re-checks every 15 seconds while visible and when the page becomes visible again.
 * `fade` stays the viewed channel's own overlay setting. `top` shows only in the channel's own room.
 */
export function CoStreamChat({ initial, top, ...chat }: Omit<ComponentProps<typeof Chat>, "squad"> & { initial: Squad | null }) {
  const [squad, setSquad] = useState(initial);
  const { username } = chat;
  useEffect(() => {
    const check = () => {
      if (!document.hidden) void send<{ squad: Squad | null }>("GET", `/api/channels/${encodeURIComponent(username)}/squad`).then(r => { if (r.ok) setSquad(r.data.squad); });
    };
    const timer = setInterval(check, 15000);
    document.addEventListener("visibilitychange", check);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", check); };
  }, [username]);
  const host = squad?.mode === "MERGED" ? squad.members.find(m => m.host)?.username : undefined;
  // Keyed by room, so switching rooms resets the socket and history.
  return squad && host ? <Chat key={`squad:${squad.id}`} {...chat} username={host} squad={squad.id} /> : <Chat key="channel" {...chat} top={top} />;
}
