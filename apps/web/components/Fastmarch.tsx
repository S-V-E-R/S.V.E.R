"use client";
import { useEffect, useRef, useState } from "react";
import { factions, type Faction } from "../lib/factions";
import { newGame, step, tickMs, type Dir, type Game } from "../lib/fastmarch";
import "../styles/fastmarch.css";

const CELL = 12;
const KEYS: Record<string, Dir> = { ArrowUp: 0, w: 0, W: 0, ArrowRight: 1, d: 1, D: 1, ArrowDown: 2, s: 2, S: 2, ArrowLeft: 3, a: 3, A: 3 };
type Phase = "ready" | "running" | "paused" | "over";

/** Solo Fastmarch (docs/FASTMARCH.md): the player's courier against one from each other faction. No records yet. */
export function Fastmarch({ faction }: { faction: Faction | null }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const game = useRef<Game | null>(null);
  const turns = useRef<Dir[]>([]);
  const phaseRef = useRef<Phase>("ready");
  const [phase, setPhaseState] = useState<Phase>("ready");
  const [side, setSide] = useState<Faction | null>(faction);
  const [result, setResult] = useState("");
  const setPhase = (p: Phase) => { phaseRef.current = p; setPhaseState(p); };

  // Rider 0 is the player; the other two factions ride the computer couriers.
  const riders = side ? [factions.find(f => f.slug === side)!, ...factions.filter(f => f.slug !== side)] : [];

  function draw() {
    const g = game.current, ctx = canvas.current?.getContext("2d");
    if (!g || !ctx) return;
    ctx.fillStyle = "#0B0E16";
    ctx.fillRect(0, 0, g.w * CELL, g.h * CELL);
    // MAGNet's lanes, with brighter junctions where they cross.
    ctx.fillStyle = "rgba(255,255,255,.05)";
    for (let x = 0; x < g.w; x += 6) ctx.fillRect(x * CELL + CELL / 2, 0, 1, g.h * CELL);
    for (let y = 0; y < g.h; y += 6) ctx.fillRect(0, y * CELL + CELL / 2, g.w * CELL, 1);
    ctx.fillStyle = "rgba(255,255,255,.18)";
    for (let x = 0; x < g.w; x += 6) for (let y = 0; y < g.h; y += 6) ctx.fillRect(x * CELL + CELL / 2 - 1, y * CELL + CELL / 2 - 1, 3, 3);
    for (let i = 0; i < g.cells.length; i++) {
      const c = g.cells[i], x = (i % g.w) * CELL, y = Math.floor(i / g.w) * CELL;
      if (c === 1) { // the Static: grey cells with a fixed speckle
        ctx.fillStyle = "#2A2D35"; ctx.fillRect(x, y, CELL, CELL);
        ctx.fillStyle = "#5B5F6A"; ctx.fillRect(x + (i * 7) % 9, y + (i * 13) % 9, 2, 2);
      } else if (c >= 2) {
        ctx.globalAlpha = g.riders[c - 2].alive ? 1 : 0.35;
        ctx.fillStyle = riders[c - 2]?.color ?? "#A3A6AE"; ctx.fillRect(x + 1, y + 1, CELL - 2, CELL - 2);
        ctx.globalAlpha = 1;
      }
    }
    for (const r of g.riders) if (r.alive) { ctx.fillStyle = "#FFFFFF"; ctx.fillRect(r.x * CELL + 2, r.y * CELL + 2, CELL - 4, CELL - 4); }
  }

  function start() {
    game.current = newGame((Math.random() * 2 ** 31) | 0);
    turns.current = [];
    setResult("");
    setPhase("running");
  }

  // The loop runs only while running; it pauses itself when the tab is hidden.
  useEffect(() => {
    if (phase !== "running") { game.current ??= newGame(7); draw(); return; }
    canvas.current?.focus();
    let frame = 0, last = performance.now(), carry = 0;
    const loop = (now: number) => {
      const g = game.current!;
      carry += Math.min(now - last, 250); last = now;
      let moved = false;
      while (carry >= tickMs(g.tick) && g.riders[0].alive) { carry -= tickMs(g.tick); step(g, turns.current.shift()); moved = true; }
      if (moved) draw();
      if (!g.riders[0].alive) {
        const seconds = Array.from({ length: g.tick }, (_, t) => tickMs(t)).reduce((a, b) => a + b, 0) / 1000;
        const outran = g.riders.slice(1).flatMap((r, i) => r.alive ? [] : [riders[i + 1].name]);
        setResult(`Lost to the Static after ${seconds.toFixed(1)} seconds.${outran.length ? ` You outlasted ${outran.join(" and ")}.` : ""}`);
        setPhase("over");
        return;
      }
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    const hide = () => { if (document.hidden) setPhase("paused"); };
    document.addEventListener("visibilitychange", hide);
    return () => { cancelAnimationFrame(frame); document.removeEventListener("visibilitychange", hide); };
    // draw and riders only read the current game and side; the loop restarts when the phase changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, side]);

  function queue(d: Dir) {
    if (phaseRef.current !== "running") return;
    if (turns.current.length < 2 && turns.current.at(-1) !== d) turns.current.push(d);
  }
  function onKey(event: React.KeyboardEvent) {
    if (event.key === " " || event.key === "p" || event.key === "P") {
      if (phaseRef.current === "running") setPhase("paused"); else if (phaseRef.current === "paused") setPhase("running");
      event.preventDefault();
    } else if (event.key in KEYS) { queue(KEYS[event.key]); event.preventDefault(); }
  }
  const swipe = useRef<{ x: number; y: number } | null>(null);
  function onPointerUp(event: React.PointerEvent) {
    const from = swipe.current; swipe.current = null;
    if (!from) return;
    const dx = event.clientX - from.x, dy = event.clientY - from.y;
    if (Math.max(Math.abs(dx), Math.abs(dy)) < 20) return;
    queue(Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 1 : 3) : (dy > 0 ? 2 : 0));
  }

  return <section className="fastmarch frame" aria-labelledby="fastmarch-title">
    <header className="fastmarch-head">
      <h2 id="fastmarch-title">Fastmarch</h2>
      <p>Carry the signal back out. Steer with the arrow keys or WASD, or swipe. Space pauses. Don&apos;t hit a trail, the wall or the Static.</p>
      {side && <ul className="fastmarch-riders" aria-label="Couriers">{riders.map((f, i) =>
        <li key={f.slug}><span className="fastmarch-swatch" style={{ background: f.color }} aria-hidden="true" />{i === 0 ? `You (${f.name})` : f.name}</li>)}</ul>}
    </header>
    <div className="fastmarch-stage">
      <canvas ref={canvas} width={60 * CELL} height={36 * CELL} tabIndex={0} className="fastmarch-canvas"
        role="img" aria-label="Fastmarch race. Use the arrow keys to steer."
        onKeyDown={onKey}
        onPointerDown={event => { swipe.current = { x: event.clientX, y: event.clientY }; }}
        onPointerUp={onPointerUp} />
      {phase !== "running" && <div className="fastmarch-overlay">
        {phase === "paused" && <button className="button" onClick={() => setPhase("running")}>Resume</button>}
        {phase !== "paused" && (side
          ? <>
            <button className="button" onClick={start}>{phase === "over" ? "Run again" : "Play"}</button>
            {!faction && phase === "over" && <button className="quiet" onClick={() => setSide(null)}>Change side</button>}
          </>
          : <div className="fastmarch-sides" role="group" aria-label="Pick a side to ride for">{factions.map(f =>
            <button key={f.slug} className="button" style={{ borderColor: f.color }} onClick={() => setSide(f.slug)}>Ride for {f.name}</button>)}</div>)}
      </div>}
    </div>
    <p className="fastmarch-result" aria-live="polite">{result}</p>
  </section>;
}
