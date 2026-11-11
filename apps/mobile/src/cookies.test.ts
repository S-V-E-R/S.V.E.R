import assert from "node:assert/strict";
import test from "node:test";
import { applySetCookie, cookieHeader, setCookieLines } from "./cookies.ts";

test("session and challenge cookies are stored and a cleared cookie is dropped", () => {
  const headers = new Headers();
  headers.append("set-cookie", "sver_dev=session-token; Path=/; HttpOnly; SameSite=Lax");
  headers.append("set-cookie", "sver_dev_mfa=browser; Max-Age=300; Path=/; HttpOnly");
  const jar = applySetCookie({}, headers);
  assert.equal(jar.sver_dev, "session-token");
  assert.equal(jar.sver_dev_mfa, "browser");
  assert.equal(cookieHeader(jar), "sver_dev=session-token; sver_dev_mfa=browser");

  const cleared = new Headers();
  cleared.append("set-cookie", "sver_dev_mfa=; Max-Age=0; Path=/");
  const next = applySetCookie(jar, cleared);
  assert.equal(next.sver_dev, "session-token");
  assert.equal(next.sver_dev_mfa, undefined);
});

test("a combined Set-Cookie header still yields one line per cookie", () => {
  const headers = { get: () => "sver_dev=abc; Path=/, sver_dev_challenge=def; Path=/", getSetCookie: () => [] as string[] };
  assert.deepEqual(setCookieLines(headers), ["sver_dev=abc; Path=/", " sver_dev_challenge=def; Path=/"]);
});
