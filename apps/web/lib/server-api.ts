import { cookies } from "next/headers";

export const apiOrigin = () => process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080";

/** Server-side API read that forwards the visitor's cookies and is never cached or shared. */
export async function apiGet<T = Record<string, unknown>>(path: string): Promise<{ status: number; data: T | null }> {
  const jar = await cookies();
  try {
    const response = await fetch(`${apiOrigin()}${path}`, { headers: { cookie: jar.toString() }, cache: "no-store" });
    const data = response.ok ? ((await response.json()) as T) : null;
    return { status: response.status, data };
  } catch {
    return { status: 503, data: null };
  }
}
