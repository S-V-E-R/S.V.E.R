import { useEffect } from "react";

/** Fetch on mount and on an interval. State updates happen after the request resolves. */
export function useLoad(load: () => Promise<unknown>, ms?: number) {
  useEffect(() => {
    void load();
    if (!ms) return;
    const timer = setInterval(() => { void load(); }, ms);
    return () => clearInterval(timer);
  }, [load, ms]);
}
