// This browser's Private-mode device (docs/COMMUNITY.md "Private mode"): a key pair made once, the
// private key kept (non-exportable) in IndexedDB, the public key registered with the server.
import { send } from "./client-api";
import { exportPublic, newDeviceKeys } from "./e2e";

export type ThisDevice = { id: string; privateKey: CryptoKey };

function store(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("sver-e2e", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("devices");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
async function run<T>(mode: IDBTransactionMode, work: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await store();
  return new Promise((resolve, reject) => {
    const request = work(db.transaction("devices", mode).objectStore("devices"));
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/**
 * The device for this account in this browser, registering one if needed. Null when the browser
 * can't keep keys (no IndexedDB or WebCrypto); a string is the server's reason (e.g. 5 devices).
 */
export async function ensureDevice(account: string): Promise<ThisDevice | string | null> {
  if (typeof indexedDB === "undefined" || !globalThis.crypto?.subtle) return null;
  try {
    const stored = await run<ThisDevice | undefined>("readonly", s => s.get(account));
    const listed = await send<{ devices: { id: string }[] }>("GET", "/api/me/dm-devices");
    if (stored && listed.ok && listed.data.devices.some(d => d.id === stored.id)) return stored;
    const keys = await newDeviceKeys();
    const added = await send<{ id: string }>("POST", "/api/me/dm-devices", { public_key: await exportPublic(keys.publicKey) });
    if (!added.ok) return added.error;
    const device = { id: added.data.id, privateKey: keys.privateKey };
    await run("readwrite", s => s.put(device, account));
    return device;
  } catch {
    return null;
  }
}
