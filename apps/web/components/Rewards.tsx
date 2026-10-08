/** A channel reward bought with Engagement Valor (docs/SUPPORT.md); shown in ChatDock. */
export type Reward = { id: string; kind: "custom" | "highlight"; name: string; cost: number; cooldown_seconds: number; per_stream_limit: number | null; prompt: string | null; enabled: boolean; ready_at: string | null };
