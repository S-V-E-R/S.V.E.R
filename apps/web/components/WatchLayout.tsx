"use client";
import { useEffect, useState, useSyncExternalStore } from "react";

type Pane = "play" | "chat";
// The viewer's choice, kept for the tab so it holds from stream to stream.
let chosen: Pane | null = null;
const listeners = new Set<() => void>();
function saved(): Pane | null {
  if (chosen) return chosen;
  try { const value = sessionStorage.getItem("watch-pane"); return value === "play" || value === "chat" ? value : null; } catch { return null; }
}
function choose(pane: Pane) {
  chosen = pane;
  try { sessionStorage.setItem("watch-pane", pane); } catch { /* storage blocked: this page only */ }
  listeners.forEach(listener => listener());
}
function subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }

/**
 * The watch page's frame. On phones (under 960 px) a Play / Chat switch under the player shows
 * the stream's panels (Plays, CrowdSync, the board) or chat, one at a time, so both are one tap
 * from the player; wider screens show both and no switch (styles/discovery.css).
 * `play`: the channel has a board or Plays, so its panels open first.
 */
export function WatchLayout({ play, children }: { play: boolean; children: React.ReactNode }) {
  const pane = useSyncExternalStore(subscribe, saved, () => null) ?? (play ? "play" : "chat");
  const [unread, setUnread] = useState(false);
  useEffect(() => {
    const onMessage = () => { if (pane !== "chat") setUnread(true); };
    window.addEventListener("sver:chat-message", onMessage);
    return () => window.removeEventListener("sver:chat-message", onMessage);
  }, [pane]);
  function show(next: Pane) {
    choose(next);
    if (next !== "chat") return;
    setUnread(false);
    // Chat was hidden, so its list couldn't follow new messages: open it at the newest.
    requestAnimationFrame(() => { const list = document.querySelector(".watch > .chat .chat-messages"); if (list) list.scrollTop = list.scrollHeight; });
  }
  return <div className="channel watch" data-pane={pane}>
    <div className="watch-switch" role="group" aria-label="Show">
      <button type="button" className={pane === "play" ? "small" : "small quiet"} aria-pressed={pane === "play"} onClick={() => show("play")}>Play</button>
      <button type="button" className={pane === "chat" ? "small" : "small quiet"} aria-pressed={pane === "chat"} onClick={() => show("chat")}>Chat{unread && pane !== "chat" && <span className="nav-dot" aria-label="New messages" />}</button>
    </div>
    {children}
  </div>;
}
