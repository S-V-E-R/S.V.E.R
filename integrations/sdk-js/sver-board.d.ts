// Types for sver-board.mjs (the S.V.E.R board SDK).
export const DEFAULT_GATEWAY: string;

export type Control = {
  id: string; kind: "button" | "label" | "text" | "goal" | "joystick" | "rally"; label: string;
  cost: number; cooldown_seconds: number; per_stream_limit: number | null;
  audience: "everyone" | "followers" | "subscribers" | "moderators"; effect: string; target: number | null; width: number;
};
export type Board = { screens: { name: string; controls: Control[] }[] };
export type ControlState = { label?: string; disabled?: boolean };
export type Snapshot = { board: Board | null; version: number; disabled: boolean; state: Record<string, ControlState>; goals: Record<string, number> };
export type Hello = Snapshot & { type: "hello"; kind: "bridge" | "game"; channel: string; protocol: number };
export type Press = {
  type: "board_effect"; control: string; label: string; effect: string; user: { username: string };
  text?: string | null; goal?: { progress: number; target: number; reached: boolean } | null; stream_ms: number | null; at: number;
};
export type Input = { type: "board_input"; control: string; x: number; y: number; user: { username: string }; stream_ms: number | null; at: number };
export type Effect = { type: "board_effect"; effect: string; label?: string; caption?: string; skill?: string; surge?: number; user?: { username: string } };
export type StateChange = { type: "board_state"; state: Record<string, ControlState>; goals: Record<string, number> };
export type Change = { label?: string | null; disabled?: boolean; progress?: number };

export class SverBoard {
  constructor(options: { token: string; url?: string; reconnect?: boolean; WebSocket?: typeof WebSocket });
  current: Snapshot | null;
  on(type: "hello", fn: (hello: Hello) => void): () => void;
  on(type: "press", fn: (press: Press) => void): () => void;
  on(type: "input", fn: (input: Input) => void): () => void;
  on(type: "effect", fn: (effect: Effect) => void): () => void;
  on(type: "board", fn: (board: Snapshot) => void): () => void;
  on(type: "state", fn: (change: StateChange) => void): () => void;
  on(type: "error", fn: (error: Error) => void): () => void;
  on(type: "close", fn: (close: { code: number; reason: string }) => void): () => void;
  connect(): Promise<Hello>;
  setState(controls: Record<string, Change>): Promise<void>;
  disable(control: string, disabled?: boolean): Promise<void>;
  label(control: string, label: string | null): Promise<void>;
  progress(control: string, progress: number): Promise<void>;
  ping(): Promise<void>;
  close(): void;
}
