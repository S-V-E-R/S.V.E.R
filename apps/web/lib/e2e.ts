// Private mode for direct messages (docs/COMMUNITY.md "Private mode"). Everything here runs in
// the browser with WebCrypto; the server only ever sees public keys, ciphertext and a commitment.
//
// A message: {body, frank} as JSON, encrypted with a fresh AES-GCM key. That key is wrapped for
// every device of both people: ECDH (P-256) between a one-time key pair and each device's key,
// HKDF-SHA-256 to an AES-GCM key, then AES-GCM. The commitment is SHA-256(frank || body), which
// lets a recipient prove what a reported message said without the server reading anything else.

export type Device = { id: string; public_key: string };
export type Envelope = { v: 1; epk: string; iv: string; ct: string; keys: Record<string, { iv: string; k: string }> };
export type Opened = { body: string; frank: string; verified: boolean };

const ECDH = { name: "ECDH", namedCurve: "P-256" } as const;
const INFO = new TextEncoder().encode("sver-dm-v1");
const subtle = () => globalThis.crypto.subtle;
const random = (n: number) => globalThis.crypto.getRandomValues(new Uint8Array(n));

export function b64(bytes: ArrayBuffer | Uint8Array): string {
  let text = "";
  for (const byte of new Uint8Array(bytes)) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
export function unb64(value: string): Uint8Array<ArrayBuffer> {
  const text = atob(value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "="));
  return Uint8Array.from(text, c => c.charCodeAt(0));
}

/** A device key pair. The private key can't be exported, so it never leaves this browser. */
export function newDeviceKeys(): Promise<CryptoKeyPair> {
  return subtle().generateKey(ECDH, false, ["deriveBits"]) as Promise<CryptoKeyPair>;
}
export async function exportPublic(key: CryptoKey): Promise<string> {
  return b64(await subtle().exportKey("raw", key));
}
function importPublic(raw: string): Promise<CryptoKey> {
  return subtle().importKey("raw", unb64(raw), ECDH, true, []);
}
/** The key that wraps a message key between one private key and one public key. */
async function wrappingKey(privateKey: CryptoKey, publicKey: CryptoKey): Promise<CryptoKey> {
  const bits = await subtle().deriveBits({ name: "ECDH", public: publicKey }, privateKey, 256);
  const base = await subtle().importKey("raw", bits, "HKDF", false, ["deriveKey"]);
  return subtle().deriveKey({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: INFO }, base, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}
/** SHA-256(frank || body) as lowercase hex: the same as the server's `franking_hex`. */
export async function commitment(frank: Uint8Array, body: string): Promise<string> {
  const text = new TextEncoder().encode(body);
  const joined = new Uint8Array(frank.length + text.length);
  joined.set(frank);
  joined.set(text, frank.length);
  const digest = new Uint8Array(await subtle().digest("SHA-256", joined));
  return Array.from(digest, b => b.toString(16).padStart(2, "0")).join("");
}

/**
 * A safety number for a conversation: 12 groups of 5 digits from both people's device keys. Both
 * people see the same number (the sides are ordered by content, not by who's looking), and it
 * changes when either adds or removes a device. Matching numbers, compared outside S.V.E.R, mean
 * the server handed out the real keys.
 */
export async function safetyNumber(mine: Device[], theirs: Device[]): Promise<string> {
  const side = (devices: Device[]) => devices.map(d => d.public_key).sort().join("\n");
  const sides = [side(mine), side(theirs)].sort();
  const digest = new Uint8Array(await subtle().digest("SHA-512", new TextEncoder().encode(`sver-safety-v1\n${sides[0]}\n\n${sides[1]}`)));
  const groups = [];
  for (let i = 0; i < 60; i += 5) {
    const value = digest.slice(i, i + 5).reduce((n, b) => n * 256 + b, 0);
    groups.push(String(value % 100000).padStart(5, "0"));
  }
  return groups.join(" ");
}

/** Encrypts a message for every listed device (both people's). */
export async function seal(body: string, devices: Device[]): Promise<{ envelope: Envelope; commitment: string }> {
  const message = await subtle().generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
  const frank = random(32);
  const iv = random(12);
  const ct = await subtle().encrypt({ name: "AES-GCM", iv }, message, new TextEncoder().encode(JSON.stringify({ body, frank: b64(frank) })));
  const raw = await subtle().exportKey("raw", message);
  // The one-time private key can't be exported either (its public half always can be).
  const once = await subtle().generateKey(ECDH, false, ["deriveBits"]) as CryptoKeyPair;
  const keys: Envelope["keys"] = {};
  for (const device of devices) {
    // A device whose key isn't a valid point is skipped rather than breaking every send.
    const publicKey = await importPublic(device.public_key).catch(() => null);
    if (!publicKey) continue;
    const wrap = await wrappingKey(once.privateKey, publicKey);
    const keyIv = random(12);
    keys[device.id] = { iv: b64(keyIv), k: b64(await subtle().encrypt({ name: "AES-GCM", iv: keyIv }, wrap, raw)) };
  }
  return { envelope: { v: 1, epk: await exportPublic(once.publicKey), iv: b64(iv), ct: b64(ct), keys }, commitment: await commitment(frank, body) };
}

/**
 * Decrypts a message on this device, or null when it wasn't sent to this device, was altered, or
 * doesn't match its commitment (a sender can't show one text and commit to another for reports).
 */
export async function open(envelope: Envelope, expected: string, deviceId: string, privateKey: CryptoKey): Promise<Opened | null> {
  const entry = envelope.keys?.[deviceId];
  if (!entry) return null;
  try {
    const wrap = await wrappingKey(privateKey, await importPublic(envelope.epk));
    const raw = await subtle().decrypt({ name: "AES-GCM", iv: unb64(entry.iv) }, wrap, unb64(entry.k));
    const message = await subtle().importKey("raw", raw, "AES-GCM", false, ["decrypt"]);
    const plain = await subtle().decrypt({ name: "AES-GCM", iv: unb64(envelope.iv) }, message, unb64(envelope.ct));
    const { body, frank } = JSON.parse(new TextDecoder().decode(plain)) as { body: string; frank: string };
    if (await commitment(unb64(frank), body) !== expected) return null;
    return { body, frank, verified: true };
  } catch {
    return null;
  }
}
