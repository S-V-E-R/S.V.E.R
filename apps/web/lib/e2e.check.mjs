// Private-mode crypto check (lib/e2e.ts), run with plain node 24: node lib/e2e.check.mjs
import assert from "node:assert/strict";
import { b64, commitment, exportPublic, newDeviceKeys, open, seal, unb64 } from "./e2e.ts";

// The commitment matches the server's franking_hex (SHA-256 of the secret bytes, then the text).
assert.equal(await commitment(new Uint8Array(32).fill(7), "secret plan"), "1835443b354e6fb2f02314feb751e1655cb0b60ecb64503520c719c922c1cea0");
assert.deepEqual([...unb64(b64(new Uint8Array([0, 250, 255, 1])))], [0, 250, 255, 1]);

// Alice's two devices and Bob's one: everyone listed can read it, nobody else can.
const [alice1, alice2, bob, eve] = await Promise.all([newDeviceKeys(), newDeviceKeys(), newDeviceKeys(), newDeviceKeys()]);
const devices = [
  { id: "a1", public_key: await exportPublic(alice1.publicKey) },
  { id: "a2", public_key: await exportPublic(alice2.publicKey) },
  { id: "b1", public_key: await exportPublic(bob.publicKey) },
];
assert.equal(unb64(devices[0].public_key).length, 65, "the server expects an uncompressed P-256 point");
const { envelope, commitment: committed } = await seal("meet at the hill ☀", devices);
assert.equal(envelope.v, 1);
assert.equal(unb64(envelope.keys.b1.k).length, 48, "a wrapped 32-byte key plus its tag");
for (const [id, keys] of [["a1", alice1], ["a2", alice2], ["b1", bob]]) {
  const opened = await open(envelope, committed, id, keys.privateKey);
  assert.equal(opened?.body, "meet at the hill ☀", id);
  assert.equal(opened?.verified, true);
}
assert.equal(await open(envelope, committed, "e1", eve.privateKey), null, "not sent to this device");
assert.equal(await open(envelope, committed, "b1", eve.privateKey), null, "the wrong private key can't unwrap it");
// A changed ciphertext fails to open; a wrong commitment opens but isn't verified.
const tampered = { ...envelope, ct: b64(unb64(envelope.ct).map((b, i) => i === 0 ? b ^ 1 : b)) };
assert.equal(await open(tampered, committed, "b1", bob.privateKey), null);
assert.equal((await open(envelope, "0".repeat(64), "b1", bob.privateKey))?.verified, false);
// The evidence a report sends matches the stored commitment.
const opened = await open(envelope, committed, "b1", bob.privateKey);
assert.equal(await commitment(unb64(opened.frank), opened.body), committed);
console.log("e2e: ok");
