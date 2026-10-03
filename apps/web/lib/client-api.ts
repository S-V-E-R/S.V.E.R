"use client";
import { useEffect } from "react";
export type Result<T> = { ok: true; data: T } | { ok: false; status: number; error: string; field?: string };

/** Browser API call: same-origin, never cached, field-level errors passed through. */
export async function send<T = Record<string, unknown>>(method: string, path: string, body?: unknown): Promise<Result<T>> {
  try {
    const form = typeof FormData !== "undefined" && body instanceof FormData;
    const response = await fetch(path, { method, credentials: "same-origin", cache: "no-store", headers: body === undefined || form ? {} : { "Content-Type": "application/json" }, body: body === undefined ? undefined : form ? (body as FormData) : JSON.stringify(body) });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) return { ok: false, status: response.status, error: data.error || (response.status === 413 ? "That file is too large." : "The request could not be completed."), field: data.field };
    return { ok: true, data: data as T };
  } catch {
    return { ok: false, status: 0, error: "The service could not be reached. Please try again." };
  }
}

/** Runs an async loader on mount and whenever it changes. Loaders set state only after awaiting the
 *  network, which react-hooks/set-state-in-effect cannot tell apart from a synchronous update. */
export function useLoad(load: () => Promise<unknown>) {
  useEffect(() => { void load(); }, [load]);
}
