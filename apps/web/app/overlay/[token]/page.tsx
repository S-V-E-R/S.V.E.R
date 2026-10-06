"use client";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { EffectStage, useShown, type BoardEvent } from "../../../components/BoardEffects";

/**
 * The streamer's OBS browser source (docs/CROWDSYNC.md "Outputs"): board effects drawn inside the
 * video, so every viewer sees them in step with the picture. The private token in the URL is the
 * only credential; it reaches effects and nothing else. Reconnects on its own.
 */
export default function Overlay() {
  const { token } = useParams<{ token: string }>();
  const [shown, add] = useShown();
  const [lost, setLost] = useState(false);
  useEffect(() => {
    let ws: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const connect = () => {
      ws = new WebSocket(`${location.origin.replace(/^http/, "ws")}/api/boards/overlay/ws?token=${encodeURIComponent(token)}`);
      ws.onopen = () => setLost(false);
      ws.onmessage = event => {
        const data = JSON.parse(event.data) as BoardEvent;
        if (data.type === "board_effect") add(data);
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
  </main>;
}
