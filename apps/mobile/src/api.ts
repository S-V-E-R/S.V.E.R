import { cookieHeader } from "./cookies";
import { loadJar, storeResponseCookies } from "./session";
import { absolute, appOrigin } from "./urls";

/** Same result shape as apps/web/lib/client-api.ts. The website sends a browser cookie; this app sends the stored one. */
export type Result<T> = { ok: true; data: T } | { ok: false; status: number; error: string; field?: string; retry?: number };

export async function send<T>(method: string, path: string, body?: unknown): Promise<Result<T>> {
  const jar = await loadJar();
  try {
    const headers: Record<string, string> = { Accept: "application/json", Origin: appOrigin };
    const cookie = cookieHeader(jar);
    if (cookie) headers.Cookie = cookie;
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const response = await fetch(absolute(path), {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    await storeResponseCookies(response);
    const data: unknown = await response.json().catch(() => ({}));
    if (!response.ok) {
      const record = data && typeof data === "object" ? (data as { error?: unknown; field?: unknown; retry?: unknown }) : {};
      // Seconds until a refused send may go again (slow mode, a timeout): the body's `retry`, else Retry-After.
      const retry = typeof record.retry === "number" ? record.retry : Number(response.headers.get("retry-after")) || undefined;
      return {
        ok: false,
        status: response.status,
        error: typeof record.error === "string" ? record.error : "The request could not be completed.",
        field: typeof record.field === "string" ? record.field : undefined,
        retry,
      };
    }
    return { ok: true, data: data as T };
  } catch {
    return { ok: false, status: 0, error: "The service could not be reached. Please try again." };
  }
}
