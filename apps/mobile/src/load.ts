import { useCallback } from "react";
import { useFocusEffect } from "expo-router";

/**
 * Fetch when the screen gains focus, then on an interval while it stays focused. Another tab or a
 * screen pushed on top pauses the interval. State updates happen after the request resolves.
 */
export function useLoad(load: () => Promise<unknown>, ms?: number) {
  useFocusEffect(useCallback(() => {
    void load();
    if (!ms) return;
    const timer = setInterval(() => { void load(); }, ms);
    return () => clearInterval(timer);
  }, [load, ms]));
}
