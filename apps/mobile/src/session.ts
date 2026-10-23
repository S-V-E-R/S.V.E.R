import * as SecureStore from "expo-secure-store";
import { applySetCookie, cookieHeader, type Jar } from "./cookies";

const COOKIES = "sver.cookies";
const BROWSER = "sver.browser";

let memory: Jar | null = null;

async function readStore(key: string): Promise<string | null> {
  try {
    return await SecureStore.getItemAsync(key);
  } catch {
    return null;
  }
}

async function writeStore(key: string, value: string): Promise<void> {
  try {
    await SecureStore.setItemAsync(key, value);
  } catch {
    // Secure storage can be unavailable in a restricted context. The in-memory jar still works until the app closes.
  }
}

function parseJar(raw: string | null): Jar {
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const jar: Jar = {};
    for (const [name, value] of Object.entries(parsed)) {
      if (typeof value === "string" && value.length > 0) jar[name] = value;
    }
    return jar;
  } catch {
    return {};
  }
}

export async function loadJar(): Promise<Jar> {
  if (memory) return memory;
  memory = parseJar(await readStore(COOKIES));
  return memory;
}

export async function saveJar(jar: Jar): Promise<void> {
  memory = jar;
  await writeStore(COOKIES, JSON.stringify(jar));
}

export async function clearJar(): Promise<void> {
  memory = {};
  try {
    await SecureStore.deleteItemAsync(COOKIES);
  } catch {
    await writeStore(COOKIES, "{}");
  }
}

/** Remember Set-Cookie from a response. The values are the session secret; never log them.
 *  ponytail: two responses in flight each write the whole jar, so the later one can drop a cookie the earlier one added. A single-flight write queue is the upgrade if that drops a sign-in. */
export async function storeResponseCookies(response: Response): Promise<void> {
  const current = await loadJar();
  const next = applySetCookie(current, response.headers);
  if (cookieHeader(next) !== cookieHeader(current)) await saveJar(next);
}

const BROWSER_ID = /^[A-Za-z0-9-]{16,64}$/;

export function newId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && crypto.getRandomValues) crypto.getRandomValues(bytes);
  else for (let i = 0; i < bytes.length; i += 1) bytes[i] = Math.floor(Math.random() * 256);
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = [...bytes].map(b => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Stable per-install id for playback heartbeats. Guests count once per install, never by IP. */
export async function browserId(): Promise<string> {
  const existing = await readStore(BROWSER);
  if (existing && BROWSER_ID.test(existing)) return existing;
  const id = newId();
  await writeStore(BROWSER, id);
  return id;
}
