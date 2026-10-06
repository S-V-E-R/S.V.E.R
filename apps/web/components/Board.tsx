"use client";
import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { send } from "../lib/client-api";
import { REDUCE_KEY, type BoardEvent } from "./BoardEffects";
import "../styles/boards.css";

export type Control = { id: string; kind: "button" | "label" | "text" | "goal" | "joystick"; label: string; cost: number; cooldown_seconds: number; per_stream_limit: number | null; audience: "everyone" | "followers" | "subscribers" | "moderators"; effect: string; target: number | null; width: number };
export type BoardDef = { screens: { name: string; controls: Control[] }[] };
type View = { board: BoardDef | null; version: number; disabled: boolean; live: boolean; overlay: boolean; goals: Record<string, number>; used: Record<string, number>; last_press: Record<string, string>; balance: number | null; signed_in: boolean; can_run: boolean; blocks: string[] | null };
const AUDIENCE = { everyone: "", followers: "Followers", subscribers: "Subscribers", moderators: "Moderators" };
const STICK: [string, number, number][] = [["↑", 0, -1], ["←", -1, 0], ["→", 1, 0], ["↓", 0, 1]];

/**
 * A channel's CrowdSync board under the player (docs/CROWDSYNC.md "Boards"). It loads only when
 * opened and shares the chat socket for updates (via Chat's "sver:board" events).
 */
export function Board({ username }: { username: string }) {
  const path = `/api/channels/${encodeURIComponent(username)}/board`;
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<View | null>(null);
  const [screen, setScreen] = useState(0);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [calm, setCalm] = useState(false);
  const [blockName, setBlockName] = useState("");
  const lastMove = useRef(0);
  const load = useCallback(async () => {
    const r = await send<View>("GET", path);
    if (r.ok) setView(r.data); else setNote(r.error);
  }, [path]);
  // Nothing loads until the viewer opens the panel.
  function toggle(opened: boolean) {
    setOpen(opened);
    if (!opened) return;
    try { setCalm(localStorage.getItem(REDUCE_KEY) === "1"); } catch { /* storage unavailable */ }
    void load();
  }
  useEffect(() => {
    if (!open) return;
    const tick = setInterval(() => setNow(Date.now()), 1000);
    const on = (event: Event) => {
      const { channel, event: e } = (event as CustomEvent<{ channel: string; event: BoardEvent }>).detail;
      if (channel !== username.toLowerCase()) return;
      if (e.type === "board") void load();
      else if (e.type === "board_effect" && e.goal && e.control) {
        const id = e.control, progress = e.goal.progress;
        setView(v => v && { ...v, goals: { ...v.goals, [id]: progress } });
      }
    };
    window.addEventListener("sver:board", on);
    return () => { clearInterval(tick); window.removeEventListener("sver:board", on); };
  }, [open, username, load]);

  async function press(control: Control, extra: Record<string, unknown> = {}) {
    if (!view) return;
    setBusy(control.kind !== "joystick"); setNote("");
    const r = await send<{ balance?: number }>("POST", `${path}/press`, { id: crypto.randomUUID(), version: view.version, control: control.id, ...extra });
    setBusy(false);
    if (!r.ok) {
      setNote(r.error);
      if (r.status === 409) void load();
      return;
    }
    if (control.kind === "joystick") return;
    setView(v => v && { ...v, balance: r.data.balance ?? v.balance, last_press: { ...v.last_press, [control.id]: new Date().toISOString() }, used: { ...v.used, [control.id]: (v.used[control.id] ?? 0) + 1 } });
    if (control.kind === "text") setTexts(t => ({ ...t, [control.id]: "" }));
  }
  function move(control: Control, x: number, y: number, at: number) {
    // The server allows 10 moves a second; stay under it (`at` is the click's timestamp).
    if (at - lastMove.current < 120) return;
    lastMove.current = at;
    void press(control, { x, y });
  }
  async function run(method: string, url: string, body?: unknown) {
    setNote("");
    const r = await send(method, url, body);
    if (r.ok) { setBlockName(""); await load(); } else setNote(r.error);
  }
  function setReduce(on: boolean) {
    setCalm(on);
    try { if (on) localStorage.setItem(REDUCE_KEY, "1"); else localStorage.removeItem(REDUCE_KEY); } catch { /* storage unavailable */ }
  }

  const board = view?.board;
  const current = board?.screens[Math.min(screen, board.screens.length - 1)];
  const canPress = !!view && view.signed_in && view.live && !view.disabled && view.balance !== null;
  function blocker(c: Control): string | null {
    if (!view) return "";
    const last = view.last_press[c.id];
    const ready = last ? Date.parse(last) + c.cooldown_seconds * 1000 : 0;
    if (c.cooldown_seconds && ready > now) return `${Math.ceil((ready - now) / 1000)}s`;
    if (c.per_stream_limit && (view.used[c.id] ?? 0) >= c.per_stream_limit) return "Limit reached";
    if (c.target && (view.goals[c.id] ?? 0) >= c.target) return "Complete";
    if (c.cost && (view.balance ?? 0) < c.cost) return "Not enough";
    return null;
  }
  const price = (c: Control) => (c.cost ? `${c.cost.toLocaleString()} EV` : "Free") + (AUDIENCE[c.audience] ? ` · ${AUDIENCE[c.audience]}` : "");

  return <details className="board-panel frame" onToggle={e => toggle(e.currentTarget.open)}>
    <summary>Board{view?.balance != null && <> · <strong>{view.balance.toLocaleString()}</strong> Engagement Valor</>}</summary>
    {!view ? <p className="loading">{note || "Loading…"}</p> : !board ? <p className="muted">This channel has no board right now.</p> : <>
      <p className="muted small">{view.disabled ? "The board is paused." : !view.live ? "The board works while the stream is live." : !view.signed_in ? <><Link href="/login">Sign in</Link> and watch to use the board.</> : view.balance === null ? "This is your board. Try it in Creator Studio's test mode." : "Presses spend this channel's Engagement Valor, earned by watching and chatting."}</p>
      {board.screens.length > 1 && <div className="board-screens" role="tablist">{board.screens.map((s, i) => <button key={s.name + i} type="button" role="tab" aria-selected={i === screen} className={i === screen ? "small" : "small quiet"} onClick={() => setScreen(i)}>{s.name}</button>)}</div>}
      {current && <div className="board-grid">{current.controls.map(c => {
        const span = { gridColumn: `span ${c.width}` };
        const why = blocker(c);
        const off = !canPress || busy || !!why;
        if (c.kind === "label") return <p key={c.id} className="board-label" style={span}>{c.label}</p>;
        if (c.kind === "joystick") return <div key={c.id} style={span}><span className="small">{c.label}</span><div className="board-stick" role="group" aria-label={c.label}>{STICK.map(([arrow, x, y]) => <button key={arrow} type="button" className="small quiet" disabled={!canPress} aria-label={`${c.label} ${arrow}`} onClick={e => move(c, x, y, e.timeStamp)}>{arrow}</button>)}</div></div>;
        if (c.kind === "text") return <form key={c.id} style={span} onSubmit={e => { e.preventDefault(); void press(c, { text: texts[c.id] ?? "" }); }}>
          <label className="field"><span>{c.label} <small className="muted">{price(c)}</small></span><input value={texts[c.id] ?? ""} maxLength={200} disabled={!canPress} onChange={e => setTexts(t => ({ ...t, [c.id]: e.target.value }))} /></label>
          <button className="small" disabled={off || !(texts[c.id] ?? "").trim()}>{why ?? "Send"}</button>
        </form>;
        if (c.kind === "goal") return <div key={c.id} className="board-goal" style={span}>
          <span>{c.label} · {(view.goals[c.id] ?? 0).toLocaleString()} / {c.target?.toLocaleString()}</span>
          <progress max={c.target ?? 1} value={view.goals[c.id] ?? 0} aria-label={c.label} />
          <button type="button" className="small" disabled={off} onClick={() => press(c)}>{why ?? `Add (${price(c)})`}</button>
        </div>;
        return <button key={c.id} type="button" style={span} disabled={off} onClick={() => press(c)}>{c.label}<small>{why ?? price(c)}</small></button>;
      })}</div>}
      <label className="checkbox"><input type="checkbox" checked={calm} onChange={e => setReduce(e.target.checked)} /> Reduce effects</label>
      {view.can_run && <div className="board-run">
        <button type="button" className="small quiet" onClick={() => run("PUT", `${path}/disabled`, { disabled: !view.disabled })}>{view.disabled ? "Resume the board" : "Pause all effects (panic)"}</button>
        <form className="row" onSubmit={e => { e.preventDefault(); if (blockName.trim()) void run("PUT", `${path}/blocks/${encodeURIComponent(blockName.trim())}`); }}>
          <label className="field narrow"><span>Block from the board</span><input value={blockName} maxLength={26} placeholder="Username" onChange={e => setBlockName(e.target.value)} /></label>
          <button className="small quiet">Block</button>
        </form>
        {!!view.blocks?.length && <ul className="list">{view.blocks.map(name => <li key={name}>{name} <button type="button" className="link-button" onClick={() => run("DELETE", `${path}/blocks/${encodeURIComponent(name)}`)}>Unblock</button></li>)}</ul>}
      </div>}
      {note && <p role="status" className="form-message">{note}</p>}
    </>}
  </details>;
}
