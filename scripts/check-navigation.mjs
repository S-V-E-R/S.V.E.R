// Run against a built local frontend while the isolated development API/DB are running.
// Creates one synthetic account/session and removes only that account afterward.
import assert from "node:assert/strict";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";

const origin = process.argv[2] || "http://127.0.0.1:13001";
const url = new URL(origin);
assert(url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname));
const config = await fetch(`${origin}/api/auth/config`).then(r => r.json());
assert.equal(config.development, true, "Refuse a production frontend/API");
const id = randomUUID(), token = randomBytes(32).toString("base64url");
const username = `Nav_${id.replaceAll("-", "").slice(0, 16)}`;
const oldName = `navold_${id.replaceAll("-", "").slice(0, 12)}`;
function sql(statement) {
  // NAV_LOCAL_PSQL=1 uses a local psql with DATABASE_URL (isolated sver_rebuild only) instead of the dev container.
  const local = process.env.NAV_LOCAL_PSQL === "1";
  if (local) assert.match(process.env.DATABASE_URL || "", /^postgres(ql)?:\/\/[^@]+@(localhost|127\.0\.0\.1)(:\d+)?\/sver_rebuild$/);
  const result = local
    ? spawnSync("psql", [process.env.DATABASE_URL, "-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"], { input: statement, encoding: "utf8" })
    : spawnSync("docker", ["exec", "-i", "sver-rebuild-postgres-1", "psql", "-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1", "-U", "sver_rebuild", "-d", "sver_rebuild"], { input: statement, encoding: "utf8" });
  assert.equal(result.status, 0, "Local synthetic database operation failed");
  return result.stdout.trim();
}
const get = (path, cookie = "") => fetch(`${origin}${path}`, { headers: cookie ? { cookie } : {}, redirect: "manual" });
const navigation = html => html.match(/<header\b[\s\S]*?<\/header>/)?.[0] + html.match(/<aside\b[\s\S]*?<\/aside>/)?.[0];
try {
  sql(`BEGIN; INSERT INTO users(id,email,username) VALUES('${id}','${id}@example.invalid','${username}'); INSERT INTO sessions(id,user_id,token_hash,auth_version,user_agent) VALUES('${randomUUID()}','${id}','${createHash("sha256").update(token).digest("hex")}',0,'Local navigation acceptance'); INSERT INTO username_holds(handle_canonical,user_id,released_at,redirect) VALUES('${oldName}','${id}',now()+interval '1 day',true); COMMIT;`);
  const guest = await get("/login");
  assert.equal(guest.status, 200);
  assert.match(navigation(await guest.text()), /href="\/login"/);
  assert.match(navigation(await (await get("/login", "sver_dev=invalid")).text()), /href="\/signup"/);
  // Explicit auth routes (formerly the [screen] catch-all) and the channel placeholder.
  for (const path of ["/signup", "/oauth-signup", "/forgot", "/reset", "/verify", "/mfa"]) assert.equal((await get(path)).status, 200, `${path} must render signed out`);
  const guestAccount = await get("/account");
  assert.equal(guestAccount.status, 307);
  assert.equal(guestAccount.headers.get("location"), "/login");
  for (const path of ["/no_such_channel", "/support", "/SVER", `/${username}/not-a-tab`, "/admin/reports"]) {
    const channel = await get(path);
    assert.equal(channel.status, 404, `${path} must return a 404`);
    if (path !== "/admin/reports") assert.match(await channel.text(), /This channel doesn(&#x27;|')t exist\./);
  }
  // Module 2 channel routing: public page, metadata, canonical casing, aliases, tabs, holds.
  const page = await get(`/${username}`);
  assert.equal(page.status, 200, "An eligible channel renders for guests");
  const html = await page.text();
  assert.match(html, new RegExp(`@${username}`));
  assert.match(html, /<meta property="og:title"/);
  assert.match(html, new RegExp(`<link rel="canonical" href="https://sver\\.tv/${username}"`));
  assert.match(html, /Log in to follow/);
  assert.doesNotMatch(html, /Edit profile/);
  const expectRedirect = async (path, status, location) => {
    const response = await get(path);
    assert.equal(response.status, status, `${path} -> ${status}`);
    assert.equal(new URL(response.headers.get("location"), origin).pathname + new URL(response.headers.get("location"), origin).search, location, `${path} location`);
    return response;
  };
  await expectRedirect(`/${username.toLowerCase()}/wall?cursor=x`, 308, `/${username}/wall?cursor=x`);
  await expectRedirect(`/s/${username}/about`, 308, `/${username}/about`);
  await expectRedirect(`/u/${username}`, 308, `/${username}`);
  await expectRedirect(`/@${username}`, 308, `/${username}`);
  await expectRedirect(`/watch/${username}`, 302, `/${username}/live`);
  await expectRedirect(`/${username}?tab=wall`, 308, `/${username}/wall`);
  await expectRedirect(`/${username}?tab=showcase`, 308, `/${username}`);
  const liveView = await get(`/${username}/live`);
  assert.equal(liveView.status, 200, "The live view renders");
  assert.match(await liveView.text(), /is offline/, "An offline channel says so on the live view");
  const held = await expectRedirect(`/${oldName}/schedule`, 302, `/${username}/schedule`);
  assert.match(held.headers.get("cache-control") || "", /no-store/);
  for (const path of ["/settings", "/settings/profile", "/studio/channel", "/studio/stream", "/studio/chat", "/following"]) {
    const response = await get(path);
    assert.equal(response.status, 307, `${path} requires sign-in`);
    assert.equal(response.headers.get("location"), "/login");
  }
  const cookie = `sver_dev=${token}`;
  const own = await (await get(`/${username}`, cookie)).text();
  assert.match(own, /Edit profile/, "The owner sees edit controls");
  assert.doesNotMatch(await (await get(`/${username}`)).text(), /Edit profile/, "Owner HTML never reaches a following anonymous request");
  assert.equal((await get("/settings", cookie)).headers.get("location"), "/settings/profile");
  assert.equal((await get("/studio", cookie)).headers.get("location"), "/studio/stream");
  const streamStudio = await get("/studio/stream", cookie);
  assert.equal(streamStudio.status, 200);
  assert.match(await streamStudio.text(), /href="\/studio\/stream"/);
  for (const path of ["/settings/profile", "/settings/blocked", "/settings/reports", "/settings/standing", "/studio/channel/song", "/studio/channel/wall", "/following"]) assert.equal((await get(path, cookie)).status, 200, `${path} renders signed in`);
  assert.equal((await get("/admin/reports", cookie)).status, 404, "Non-staff get the admin 404");
  const account = await get("/account", cookie);
  assert.equal(account.status, 200);
  const signedIn = navigation(await account.text());
  assert.match(signedIn, new RegExp(username));
  assert.doesNotMatch(signedIn, /href="\/(login|signup)"/);
  for (const path of ["/", "/login", "/signup"]) {
    const response = await get(path, cookie);
    assert.equal(response.status, 307);
    assert.equal(response.headers.get("location"), "/account");
  }
  const error = new URLSearchParams({ error: "Provider sign-in was cancelled." });
  assert.equal((await get(`/login?${error}`, cookie)).headers.get("location"), `/account?${error}`);
  // Authenticated HTML must never leak into a following anonymous response.
  const after = await (await get("/login")).text();
  assert.doesNotMatch(navigation(after), new RegExp(username));
  assert.match(navigation(after), /href="\/login"/);
  sql(`DELETE FROM sessions WHERE user_id='${id}';`);
  const revoked = await get("/account", cookie);
  assert.equal(revoked.status, 307);
  assert.equal(revoked.headers.get("location"), "/login");
  console.log("Navigation acceptance passed: guest, invalid cookie, explicit auth routes, channel 404 parity, channel page and metadata, casing/alias/tab/hold redirects, settings/studio/admin access, signed-in header/sidebar, redirects, no cross-user cache, revoked session.");
} finally {
  sql(`DELETE FROM username_holds WHERE handle_canonical='${oldName}'; DELETE FROM users WHERE id='${id}' AND username='${username}';`);
  assert.equal(sql(`SELECT count(*) FROM users WHERE id='${id}';`), "0");
}
