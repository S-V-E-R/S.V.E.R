"use client";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import styles from "./PlaysControls.module.css";

const commands = ["up", "down", "left", "right", "a", "b", "start", "select"] as const;
type Command = typeof commands[number];
type State = { game: string; round: number; closes_at: string; server_time: string; connected: boolean; input_mode: "chat" | "rl"; votes: { command: Command; votes: number }[]; my_vote: Command | null; can_vote: boolean; last_command: Command | null; last_chosen_at: string | null };
export function PlaysControls({ username }: { username: string }) {
  const [state, setState] = useState<State>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [remaining, setRemaining] = useState(0);
  const path = `/api/channels/${encodeURIComponent(username)}/plays`;
  const load = useCallback(async () => {
    const r = await send<State>("GET", path);
    if (r.ok) { setState(r.data); setRemaining(Math.max(0, Date.parse(r.data.closes_at) - Date.parse(r.data.server_time))); }
    else { setState(undefined); setError(r.error); }
  }, [path]);
  useLoad(load);
  useEffect(() => {
    const poll = setInterval(() => { if (!document.hidden) void load(); }, 1000);
    const clock = setInterval(() => setRemaining(n => Math.max(0, n - 250)), 250);
    return () => { clearInterval(poll); clearInterval(clock); };
  }, [load]);
  const disabled = busy || !state?.connected || !state.can_vote || !!state.my_vote || remaining <= 0;
  async function vote(command: Command) {
    if (disabled || !state) return;
    setBusy(true); setError("");
    const r = await send("POST", path, { command, round: state.round });
    if (!r.ok) setError(r.error);
    await load(); setBusy(false);
  }
  return <section className={`${styles.panel} frame`} aria-labelledby="plays-title">
    <div className={styles.heading}><div><span className="eyebrow">24/7 · SVER Plays</span><h2 id="plays-title">Play together</h2><p>{state?.game ?? "Loading the game…"}</p></div><span className="badge">{!state?.connected ? "Reconnecting" : state.input_mode === "rl" ? "Autonomous play" : "Viewer control"}</span></div>
    <p>Pick one button every five seconds. The most votes wins; ties are drawn fairly. You can also type a button’s name in chat.</p>
    <div className={styles.controls} onKeyDown={e => {
      const arrow: Record<string, Command> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };
      if (arrow[e.key] && !e.repeat) { e.preventDefault(); void vote(arrow[e.key]); }
    }}>
      <div className={styles.pad} aria-label="Game buttons">{commands.map(command => <button key={command} className={styles[command]} disabled={disabled} aria-pressed={state?.my_vote === command} aria-label={`Vote ${command}`} onClick={() => void vote(command)}>{command.toUpperCase()}<small>{state?.votes.find(v => v.command === command)?.votes ?? 0} votes</small></button>)}</div>
      <div className={styles.round}><p><strong>{(remaining / 1000).toFixed(1)}s</strong> until the next move</p><progress aria-label="Time left to vote" max={5000} value={remaining} /><p role="status">{state?.my_vote ? `You voted ${state.my_vote.toUpperCase()}.` : state?.can_vote ? "Your vote counts once, across buttons and chat." : <><Link href="/login">Sign in</Link> and verify your email to vote.</>}</p>{state?.last_command && <p className="muted">Last chosen: {state.last_command.toUpperCase()}</p>}</div>
    </div>
    <p className="small-text muted">Autonomous play keeps the game moving when no verified viewers are controlling it. Game votes do not award Valor or faction influence.</p>
    {error && <p role="alert" className="notice error">{error}</p>}
  </section>;
}
