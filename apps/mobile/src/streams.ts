import type { StreamCard } from "./types";

/**
 * MAGNet discovery order is the API's fair rotation.
 * Viewer count, followers and payments never reorder this list (docs/MAGNET.md).
 */
export function rotationOrder(items: readonly StreamCard[]): StreamCard[] {
  return [...items];
}

/** The Following list: live channels first, each group kept in the API's order (newest follow first). */
export function liveFirst<T extends { user: { live?: boolean } }>(rows: readonly T[]): T[] {
  return [...rows.filter(r => r.user.live), ...rows.filter(r => !r.user.live)];
}
