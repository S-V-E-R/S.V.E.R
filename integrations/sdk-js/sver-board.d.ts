// Types for sver-board.mjs (the S.V.E.R board SDK).
export const DEFAULT_GATEWAY: string;
export const DEFAULT_API: string;

/** The stored sign-in of a game that connects with its client ID. */
export type Grant = { access_token: string; refresh_token: string; expires_at: number };
/** Where the sign-in is kept: localStorage in browsers by default, `fileStore(path)` in Node, or your own. */
export type GrantStore = { get(): Grant | null | Promise<Grant | null>; set(grant: Grant | null): void | Promise<void> };
/** What to show the streamer: "Enter `code` at `url`" (`link` opens the page with the code filled in). */
export type SignInCode = { code: string; url: string; link: string; expiresIn: number };
/** Keeps the sign-in in a file (Node). */
export function fileStore(path: string): GrantStore;
type Common = { url?: string; reconnect?: boolean; WebSocket?: typeof WebSocket };

export type Control = {
  id: string; kind: "button" | "label" | "text" | "goal" | "joystick" | "rally"; label: string;
  cost: number; cooldown_seconds: number; per_stream_limit: number | null;
  audience: "everyone" | "followers" | "subscribers" | "moderators"; effect: string; target: number | null; width: number;
  confirm?: boolean;
  /** Buttons and text inputs only. */
  color?: "steel" | "ember" | "violet" | "gold";
  /** Buttons and text inputs only. */
  icon?: "heart" | "music" | "bell" | "shield" | "flag" | "clock" | "message" | "check" | "cross" | "ban" | "lock" | "search";
};
export type Board = { screens: { name: string; controls: Control[] }[] };
/** A board a game defines: only `id`, `kind` and `label` are required per control. */
export type BoardDefinition = { screens: { name: string; controls: (Pick<Control, "id" | "kind" | "label"> & Partial<Control>)[] }[] };
export type ControlState = { label?: string; disabled?: boolean };
export type Snapshot = { board: Board | null; version: number; disabled: boolean; state: Record<string, ControlState>; goals: Record<string, number>; starting: boolean; input_cap: number | null };
export type Hello = Snapshot & { type: "hello"; kind: "bridge" | "game"; channel: string; protocol: number };
export type Press = {
  type: "board_effect"; id: string; confirm: boolean; control: string; label: string; effect: string; user: { username: string };
  text?: string | null; goal?: { progress: number; target: number; reached: boolean } | null; stream_ms: number | null; at: number;
};
export type Input = { type: "board_input"; control: string; x: number; y: number; user: { username: string }; stream_ms: number | null; at: number };
export type Effect = { type: "board_effect"; effect: string; label?: string; caption?: string; skill?: string; surge?: number; user?: { username: string } };
export type StateChange = { type: "board_state"; state: Record<string, ControlState>; goals: Record<string, number> };
export type Result = { type: "board_result"; id: string; control: string; outcome: "captured" | "released" };
export type Change = { label?: string | null; disabled?: boolean; progress?: number };

export class SverBoard {
  /**
   * `token`: a game token pasted from Creator Studio. `clientId`: "Connect to S.V.E.R" — the app's
   * public client ID from Settings → Developer; connect() signs in at sver.tv/go (scope
   * `board:control`), calling `onCode` when the streamer has to enter a code. Never a client secret.
   */
  constructor(options: Common & ({ token: string } | { clientId: string; onCode?: (shown: SignInCode) => void; store?: GrantStore; api?: string }));
  current: Snapshot | null;
  on(type: "hello", fn: (hello: Hello) => void): () => void;
  on(type: "press", fn: (press: Press) => void): () => void;
  on(type: "input", fn: (input: Input) => void): () => void;
  on(type: "effect", fn: (effect: Effect) => void): () => void;
  on(type: "board", fn: (board: Snapshot) => void): () => void;
  on(type: "state", fn: (change: StateChange) => void): () => void;
  on(type: "result", fn: (result: Result) => void): () => void;
  on(type: "error", fn: (error: Error) => void): () => void;
  on(type: "close", fn: (close: { code: number; reason: string }) => void): () => void;
  connect(): Promise<Hello>;
  /** Disconnects and forgets the sign-in, here and on S.V.E.R; the next connect() shows a new code. */
  signOut(): Promise<void>;
  setState(controls: Record<string, Change>): Promise<void>;
  disable(control: string, disabled?: boolean): Promise<void>;
  label(control: string, label: string | null): Promise<void>;
  progress(control: string, progress: number): Promise<void>;
  ping(): Promise<void>;
  ready(): Promise<void>;
  setGroups(by: "faction", screens: Partial<Record<"myria" | "aetheron" | "glint", string>>): Promise<void>;
  setGroups(by: "random", screens: string[]): Promise<void>;
  setGroups(by: "users", screens: Record<string, string>): Promise<void>;
  setGroups(by: null): Promise<void>;
  capture(press: string): Promise<void>;
  release(press: string): Promise<void>;
  setInputCap(perSecond: number | null): Promise<void>;
  /** Makes the game's own board live while it is connected. Rejects unless the streamer allowed "May set my board". */
  define(name: string, board: BoardDefinition): Promise<void>;
  close(): void;
}
