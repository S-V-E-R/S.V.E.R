"use client";
import { useEffect, useMemo, useRef } from "react";
/** `retry`: seconds until the server takes this again (slow mode, a timeout, a rate limit). */
export type Result<T> = { ok: true; data: T } | { ok: false; status: number; error: string; field?: string; retry?: number };

// Staff pages register a prompt here (components/StepUp.tsx). When a request is refused because the
// staff window has lapsed, the prompt asks for a code or password and the request retries once.
let stepUpHandler: (() => Promise<boolean>) | null = null;
export function setStepUpHandler(handler: (() => Promise<boolean>) | null) { stepUpHandler = handler; }
const isStepUp = (result: Result<unknown>) => !result.ok && result.status === 403 && /confirm your sign-in/i.test(result.error);

/** Browser API call: same-origin, never cached, field-level errors passed through. */
export async function send<T = Record<string, unknown>>(method: string, path: string, body?: unknown): Promise<Result<T>> {
  const result = await request<T>(method, path, body);
  if (stepUpHandler && isStepUp(result) && await stepUpHandler()) return request<T>(method, path, body);
  return result;
}
async function request<T>(method: string, path: string, body?: unknown): Promise<Result<T>> {
  try {
    const form = typeof FormData !== "undefined" && body instanceof FormData;
    const response = await fetch(path, { method, credentials: "same-origin", cache: "no-store", headers: body === undefined || form ? {} : { "Content-Type": "application/json" }, body: body === undefined ? undefined : form ? (body as FormData) : JSON.stringify(body) });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) return { ok: false, status: response.status, error: data.error || (response.status === 413 ? "That file is too large." : "The request could not be completed."), field: data.field, retry: Number(data.retry ?? response.headers.get("retry-after")) || undefined };
    return { ok: true, data: data as T };
  } catch {
    return { ok: false, status: 0, error: "The service could not be reached. Please try again." };
  }
}

/** One client id per send (the server treats a repeated id as the same send): the same `key` (the draft
 *  and its options) reuses the id until the server answers definitively, so a retry after a lost
 *  response can't post or charge twice. A network failure, 5xx or rate limit keeps it. */
export function usePendingId() {
  const pending = useRef<{ id: string; key: string } | null>(null);
  return useMemo(() => ({
    id(key: string) {
      if (pending.current?.key !== key) pending.current = { id: crypto.randomUUID(), key };
      return pending.current.id;
    },
    settle(result: Result<unknown>) {
      if (result.ok || [400, 403, 422].includes(result.status) || (result.status === 409 && /removed/i.test(result.error))) pending.current = null;
    },
    clear() { pending.current = null; },
  }), []);
}

/** OBS docks share cookies but never reload, so a signed-out dock checks every 5 seconds whether the
 *  person signed in elsewhere (another dock), then goes to `to` (or reloads). */
export function useSignInElsewhere(active: boolean, to?: string) {
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(async () => {
      if (!(await request("GET", "/api/auth/me")).ok) return;
      clearInterval(timer);
      if (to) window.location.assign(to); else window.location.reload();
    }, 5000);
    return () => clearInterval(timer);
  }, [active, to]);
}

/** Runs an async loader on mount and whenever it changes. Loaders set state only after awaiting the
 *  network, which react-hooks/set-state-in-effect cannot tell apart from a synchronous update. */
export function useLoad(load: () => Promise<unknown>) {
  useEffect(() => { void load(); }, [load]);
}
