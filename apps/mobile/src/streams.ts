import type { StreamCard } from "./types";

/**
 * MAGNet discovery order is the API's fair rotation.
 * Viewer count, followers and payments never reorder this list (docs/MAGNET.md).
 */
export function rotationOrder(items: readonly StreamCard[]): StreamCard[] {
  return [...items];
}
