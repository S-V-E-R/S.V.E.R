"use client";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { EffectStage, useShown, type BoardEvent } from "../../../components/BoardEffects";
import { Results, counterText, type Counter, type PollState } from "../../../components/Crowd";

/**
 * The streamer's OBS browser source (docs/CROWDSYNC.md "Outputs"): board effects drawn inside the
 * video, so every viewer sees them in step with the picture. The private token in the URL is the
 * only credential; it reaches effects and nothing else. Reconnects on its own.
 */
export default function Overlay() {
  const { token } = useParams<{ token: string }>();
  const [shown, add] = useShown();
  const [lost, setLost] = useState(false);
  const [counters, setCounters] = useState<Counter[]>([]);
  const [polls, setPolls] = useState<Partial<Record<PollState["kind"], PollState>>>({});
  useEffect(() => {
    let ws: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const connect = () => {
      ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/boards/overlay/ws?token=${encodeURIComponent(token)}`);
      ws.onopen = () => setLost(false);
      ws.onmessage = event => {
        const data = JSON.parse(event.data) as BoardEvent | { type: "counters"; counters: Counter[] } | { type: "poll"; poll: PollState };
        if (data.type === "board_effect") add(data);
        else if (data.type === "counters") setCounters(data.counters);
        else if (data.type === "poll") { const poll = data.poll; setPolls(p => ({ ...p, [poll.kind]: poll })); }
      };
      ws.onclose = () => { if (!stopped) { setLost(true); retry = setTimeout(connect, 5000); } };
    };
    connect();
    return () => { stopped = true; clearTimeout(retry); ws?.close(); };
  }, [token, add]);
  return <main className="overlay-page">
    <style>{"html,body{background:transparent!important}"}</style>
    {lost && <p className="sr-only">Reconnecting to S.V.E.R…</p>}
    <EffectStage events={shown} calm={false} />
    {(counters.length > 0 || polls.poll || polls.prediction) && <aside className="overlay-widgets">
      {counters.map(c => <p key={c.id} className="overlay-counter">{c.label}: <strong>{counterText(c)}</strong></p>)}
      {/* Running polls and predictions, and their results for a short while after. */}
      {Object.values(polls).filter(p => p && (p.status === "open" || p.status === "locked" || p.status === "ended" || p.status === "resolved")).map(p => <section key={p!.id} className="overlay-poll"><strong>{p!.question}</strong><Results poll={p!} /></section>)}
    </aside>}
  </main>;
}
