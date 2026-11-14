/* The board builder's pure logic (docs/CROWDSYNC.md "Builder and Studio layout"): undo history,
   reordering, resizing and new controls. The page and components/board-builder draw it. */
import type { Control } from "../components/Board";

export const MAX_CONTROLS = 24;
export const KIND_NAMES: Record<Control["kind"], string> = { button: "Button", label: "Label", text: "Text input", goal: "Goal", joystick: "Joystick", rally: "Faction rally" };

/** Undo history. `key` names the last edit, so a run of the same edit is one step. */
export type History<T> = { past: T[]; present: T; future: T[]; key?: string };
export const start = <T>(present: T): History<T> => ({ past: [], present, future: [] });
/** A new step. Edits with the same `key` in a row (typing in one field, one resize drag) count as one. */
export function push<T>(h: History<T>, next: T, key?: string): History<T> {
  if (key && key === h.key) return { ...h, present: next, future: [] };
  return { past: [...h.past, h.present].slice(-100), present: next, future: [], key };
}
/** Swaps the present for the server's copy of it (after a save) without adding a step. */
export const settle = <T>(h: History<T>, present: T): History<T> => ({ past: h.past, present, future: h.future });
export function undo<T>(h: History<T>): History<T> {
  if (!h.past.length) return h;
  return { past: h.past.slice(0, -1), present: h.past[h.past.length - 1], future: [h.present, ...h.future] };
}
export function redo<T>(h: History<T>): History<T> {
  if (!h.future.length) return h;
  return { past: [...h.past, h.present], present: h.future[0], future: h.future.slice(1) };
}

/** The list with the item at `from` taking the place at `to`. */
export function move<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return list;
  const copy = [...list];
  copy.splice(to, 0, ...copy.splice(from, 1));
  return copy;
}
/** A control's width (1 to 4 columns) when its right edge is dragged to `pointerX`. */
export function widthAt(left: number, pointerX: number, gridWidth: number, gap = 8): number {
  const column = (gridWidth - 3 * gap) / 4;
  return Math.min(4, Math.max(1, Math.round((pointerX - left + gap) / (column + gap))));
}
/** A control ID nobody on the board has: `base`, then `base-2`, `base-3`… (24 characters at most). */
export function freeId(base: string, taken: Set<string>): string {
  const stem = base.slice(0, 20);
  if (!taken.has(stem)) return stem;
  let n = 2;
  while (taken.has(`${stem}-${n}`)) n++;
  return `${stem}-${n}`;
}
export function fresh(kind: Control["kind"], taken: Set<string>): Control {
  let n = 1;
  while (taken.has(`${kind}-${n}`)) n++;
  return { id: `${kind}-${n}`, kind, label: KIND_NAMES[kind], cost: 0, cooldown_seconds: 0, per_stream_limit: null, audience: "everyone", effect: "none", target: kind === "goal" ? 100 : null, width: kind === "label" ? 4 : 1 };
}
/** The list with a copy of the control at `index` right after it, under a new ID. Unchanged when the screen is full. */
export function duplicate(list: Control[], index: number, taken: Set<string>): Control[] {
  const original = list[index];
  if (!original || list.length >= MAX_CONTROLS) return list;
  const copy = { ...original, id: freeId(original.id.replace(/-\d+$/, ""), taken) };
  return [...list.slice(0, index + 1), copy, ...list.slice(index + 1)];
}
