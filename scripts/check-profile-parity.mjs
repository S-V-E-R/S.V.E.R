// Local only (node scripts/check-profile-parity.mjs [origin]): synthetic dev users in the isolated sver_rebuild DB; headless Chrome checks
// the profile parity additions (song start/volume, War Council tiles, wall links/time, follow dates, Following unfollow, sponsor copy,
// setup grouping, link handles/icons, phone user-card sheet) and the parity additions P1-P9 (share, mood presets, readiness,
// setup photos, link suggestions, opt-in card, activity feed, rewards stub, header copy). Any uncaught page exception fails.
// The API must run with APP_ORIGIN set to the web origin.
import assert from "node:assert/strict";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { crc32, deflateSync } from "node:zlib";
const origin = process.argv[2] || "http://127.0.0.1:13001";
assert.match(process.env.DATABASE_URL || "", /@(localhost|127\.0\.0\.1)(:\d+)?\/sver_rebuild$/);
assert.match(origin, /^http:\/\/127\.0\.0\.1:\d+$/);
const sql = s => { const r = spawnSync("psql", [process.env.DATABASE_URL, "-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"], { input: s, encoding: "utf8" }); assert.equal(r.status, 0, r.stderr); return r.stdout.trim(); };
const mk = p => { const id = randomUUID(); return { id, token: randomBytes(32).toString("base64url"), username: `${p}_${id.replaceAll("-", "").slice(0, 10)}` }; };
const A = mk("PrA"), B = mk("PrB"), S = mk("PrS");
const sleep = ms => new Promise(r => setTimeout(r, ms));
// A small solid-color PNG for the setup photo upload.
const png = (w, h) => {
  const chunk = (type, data) => { const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const td = Buffer.concat([Buffer.from(type), data]); const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td)); return Buffer.concat([len, td, crc]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(w * 3, 90)]);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(Buffer.concat(Array(h).fill(row)))), chunk("IEND", Buffer.alloc(0))]);
};
const photoPath = `/tmp/parity-setup-${process.pid}.png`;
writeFileSync(photoPath, png(320, 240));
const chrome = spawn("google-chrome", ["--headless=new", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=9335", "--user-data-dir=/tmp/parity-chrome", "about:blank"], { stdio: "ignore" });
const results = [];
let dbg = null;
const check = (name, ok, detail = "") => { results.push({ name, ok: !!ok, detail }); if (!ok && dbg && process.env.PARITY_DEBUG) dbg().then(v => console.log("DEBUG", name, v)); };
try {
  const ins = u => `INSERT INTO users(id,email,username,email_verified,created_at) VALUES('${u.id}','${u.id}@example.invalid','${u.username}',true,now()-interval '30 days'); INSERT INTO sessions(id,user_id,token_hash,auth_version,user_agent) VALUES('${randomUUID()}','${u.id}','${createHash("sha256").update(u.token).digest("hex")}',0,'Local parity check'); INSERT INTO profiles(user_id,display_name) VALUES('${u.id}','${u.username}') ON CONFLICT (user_id) DO NOTHING;`;
  sql(`BEGIN; ${ins(A)} ${ins(B)} ${ins(S)}
    UPDATE users SET mfa_enabled=true,mfa_secret='synthetic-sealed-secret' WHERE id='${S.id}'; UPDATE sessions SET mfa_verified=true WHERE user_id='${S.id}';
    INSERT INTO staff_roles(user_id,role) VALUES('${S.id}','admin');
    INSERT INTO follows(follower_id,following_id,created_at) VALUES('${B.id}','${A.id}','2026-09-14T12:00:00Z');
    UPDATE profiles SET follower_count=1 WHERE user_id='${A.id}'; UPDATE profiles SET following_count=1 WHERE user_id='${B.id}';
    UPDATE profiles SET song_provider='youtube',song_media_id='dQw4w9WgXcQ',song_url='https://www.youtube.com/watch?v=dQw4w9WgXcQ',song_title='Test Song',song_artist='Test Artist',song_volume=35 WHERE user_id='${A.id}';
    INSERT INTO war_council(user_id,position,member_id) VALUES('${A.id}',1,'${B.id}');
    INSERT INTO wall_posts(id,wall_owner_id,author_id,body,status,created_at) VALUES('${randomUUID()}','${A.id}','${B.id}','Clip here: https://example.com/clip?x=1. Nice','APPROVED',now()-interval '2 hours');
    INSERT INTO sponsors(id,user_id,position,active,name,link,discount_code,category) VALUES('${randomUUID()}','${A.id}',1,true,'Test Sponsor','https://example.com/sponsor','SAVE10','HARDWARE');
    INSERT INTO setup_items(id,user_id,position,category,name,link) VALUES('${randomUUID()}','${A.id}',1,'CAMERA','Cam One','https://example.com/cam'),('${randomUUID()}','${A.id}',2,'MIC','Mic One',NULL),('${randomUUID()}','${A.id}',3,'CAMERA','Cam Two',NULL);
    INSERT INTO setup_items(id,user_id,position,category,legacy_category,name,link) VALUES('${randomUUID()}','${A.id}',4,'OTHER','LIGHTING','Old Key Light',NULL);
    INSERT INTO identities(provider,subject,user_id,handle) VALUES('twitch','tw-${A.id}','${A.id}','pr_twitch_handle'),('discord','${String(Date.now()).padEnd(18, "7")}','${A.id}','pr.discord');
    COMMIT;`);
  let ws;
  for (let i = 0; i < 40 && !ws; i++) { try { const t = await fetch("http://127.0.0.1:9335/json/list").then(r => r.json()); const p = t.find(x => x.type === "page"); if (p) ws = p.webSocketDebuggerUrl; } catch {} if (!ws) await sleep(250); }
  const sock = new WebSocket(ws); await new Promise(r => sock.addEventListener("open", r, { once: true }));
  let n = 0; const pending = new Map();
  const exceptions = [];
  sock.addEventListener("message", e => { const m = JSON.parse(e.data); if (m.method === "Runtime.exceptionThrown") exceptions.push(JSON.stringify(m.params.exceptionDetails).slice(0, 600)); if (process.env.PARITY_DEBUG && (m.method === "Runtime.exceptionThrown" || (m.method === "Runtime.consoleAPICalled" && m.params.type === "error"))) console.log("CONSOLE", JSON.stringify(m.params).slice(0, 1500)); if (pending.has(m.id)) { pending.get(m.id)(m.result); pending.delete(m.id); } });
  const cmd = (method, params = {}) => new Promise(r => { const i = ++n; pending.set(i, r); sock.send(JSON.stringify({ id: i, method, params })); });
  const evalv = async expr => (await cmd("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true }))?.result?.value;
  const until = async (expr, tries = 40) => { for (let i = 0; i < tries; i++) { const v = await evalv(expr); if (v) return v; await sleep(250); } return null; };
  const go = async (url, ready) => { await cmd("Page.navigate", { url: origin + url }); await sleep(300); return until(ready || "document.readyState === 'complete'"); };
  const as = async u => { await cmd("Network.deleteCookies", { name: "sver_dev", url: origin }); await cmd("Network.setCookie", { name: "sver_dev", value: u.token, url: origin, path: "/" }); };
  const shot = async name => { if (!process.env.PARITY_SHOTS) return; const r = await cmd("Page.captureScreenshot", { format: "png", captureBeyondViewport: true }); (await import("node:fs")).writeFileSync(`${process.env.PARITY_SHOTS}/${name}.png`, Buffer.from(r.data, "base64")); };
  const viewport = (w, h) => cmd("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: w < 600 });
  dbg = async () => JSON.stringify(await cmd("Runtime.evaluate", { expression: "location.href + ' | ' + document.title + ' | ' + document.body.innerText.slice(0, 400)", returnByValue: true }));
  await cmd("Page.enable"); await cmd("Network.enable"); await cmd("Runtime.enable");
  // Captures postMessage calls to the song iframe without loading third-party players.
  await cmd("Page.addScriptToEvaluateOnNewDocument", { source: `window.__copied = []; Object.defineProperty(Navigator.prototype, "clipboard", { get() { return { writeText: async t => { if (window.__clipFail) throw new Error("denied"); window.__copied.push(t); } }; } }); window.__msgs = []; Object.defineProperty(HTMLIFrameElement.prototype, "contentWindow", { get() { return { postMessage: (m, o) => window.__msgs.push([m, o]) }; } });` });
  await viewport(1440, 900);
  await as(B);

  // Song: no frame before the click; after it, autoplay + JS API, and the owner's volume (35) is sent.
  await go(`/${A.username}`, "!!document.querySelector('.song-start')");
  check("song: no iframe before click", await evalv("!document.querySelector('.song-frame')"));
  await evalv("document.querySelector('.song-start').click()");
  const src = await until("document.querySelector('.song-frame')?.getAttribute('src')");
  check("song: iframe after click autoplays with JS API", /autoplay=1/.test(src || "") && /enablejsapi=1/.test(src || "") && /origin=http/.test(src || ""), src);
  check("song: iframe allow autoplay", (await evalv("document.querySelector('.song-frame').getAttribute('allow')")) === "autoplay; encrypted-media");
  const msgs = await until("window.__msgs.some(m => m[0].includes('setVolume')) && JSON.stringify(window.__msgs)", 20);
  check("song: default volume sent to the player", msgs && msgs.includes('\\"args\\":[35]') && msgs.includes("https://www.youtube-nocookie.com"), msgs);

  await shot("channel-desktop");
  // War Council: 4-column square grid, crown on position 1, tile links to the member's channel.
  check("council: 4 columns on desktop", (await evalv("getComputedStyle(document.querySelector('.council')).gridTemplateColumns.split(' ').length")) === 4);
  check("council: crown on position 1", await evalv("!!document.querySelector('.council li:first-child .crown')"));
  check("council: tile links to channel", (await evalv("document.querySelector('.council-tile')?.getAttribute('href')")) === `/${B.username}`);
  check("council: square tiles", await evalv("(() => { const r = document.querySelector('.council li').getBoundingClientRect(); return Math.abs(r.width - r.height) <= 1; })()"));

  // Wall: URL becomes a nofollow link (trailing period excluded); relative time with exact tooltip.
  const wallLink = await evalv("(() => { const a = document.querySelector('.wall-body a'); return a && a.getAttribute('href') + '|' + a.getAttribute('rel'); })()");
  check("wall: URL linked with nofollow ugc", wallLink === "https://example.com/clip?x=1|nofollow noopener noreferrer ugc", wallLink);
  const ago = await evalv("(() => { const t = document.querySelector('.wall-post time'); return t && t.textContent + '|' + t.title; })()");
  check("wall: relative time with exact local tooltip", /^2 hours ago\|\w{3} \d+, \d{4}, .* [A-Z]{2,5}([+-]\d+)?$/.test(ago || ""), ago);
  const html = await fetch(`${origin}/${A.username}`).then(r => r.text());
  check("wall: server render labels the exact time UTC", /<time[^>]*title="\w{3} \d+, \d{4}, [^"]+ UTC"/.test(html));

  // Following page as B: date, Unfollow, Follow again.
  await go("/following", "!!document.querySelector('.person-row > button')");
  check("following: row shows follow date", /Followed Sep 14, 2026/.test(await evalv("document.querySelector('.person-row').textContent")));
  await evalv("document.querySelector('.person-row > button').click()");
  await until("document.querySelector('.person-row > button').textContent === 'Follow'");
  check("following: Unfollow removes the follow", sql(`SELECT count(*) FROM follows WHERE follower_id='${B.id}' AND following_id='${A.id}'`) === "0");
  await evalv("document.querySelector('.person-row > button').click()");
  await until("document.querySelector('.person-row > button').textContent === 'Unfollow'");
  check("following: Follow again restores it", sql(`SELECT count(*) FROM follows WHERE follower_id='${B.id}' AND following_id='${A.id}'`) === "1");

  // Public followers list shows the follow date.
  await go(`/${A.username}/followers`, "!!document.querySelector('.people li')");
  check("followers: follow date shown", /Followed/.test(await evalv("document.querySelector('.people li').textContent")));

  // About: sponsor rel + copy button; setup grouped by category in owner order.
  await go(`/${A.username}/about`, "!!document.querySelector('.setup')");
  check("sponsor: rel sponsored nofollow", (await evalv("document.querySelector('.cards a[rel]').getAttribute('rel')")) === "sponsored nofollow noopener noreferrer");
  await evalv("document.querySelector('button[aria-label*=\"discount code\"]').click()");
  check("sponsor: copy button copies the code", (await until("document.querySelector('button[aria-label*=\"discount code\"]').textContent === 'Copied'")) && (await evalv("window.__copied.join()")) === "SAVE10");
  const groups = await evalv("JSON.stringify([...document.querySelectorAll('.setup > div')].map(d => [d.querySelector('dt').textContent, d.querySelectorAll('dd').length]))");
  check("setup: grouped by category in owner order", groups === JSON.stringify([["Camera", 2], ["Mic & audio interface", 1], ["Other", 1]]), groups);
  await shot("about-desktop");
  check("setup: link rel sponsored nofollow", (await evalv("document.querySelector('.setup a').getAttribute('rel')")) === "sponsored nofollow noopener noreferrer");

  // Owner: Following empty state, Studio song fields, link handle -> URL, link icon + host.
  await as(A);
  await go("/following", "document.body.textContent.includes('Channels you follow appear here.')");
  check("following: empty state", await evalv("document.body.textContent.includes('Channels you follow appear here.')"));
  await go("/studio/channel/song", "!!document.querySelector('input[name=title]')");
  check("studio song: title/artist editable and prefilled", (await evalv("document.querySelector('input[name=title]').value + '|' + document.querySelector('input[name=artist]').value")) === "Test Song|Test Artist");
  check("studio song: Fetch details button", await evalv("[...document.querySelectorAll('button')].some(b => b.textContent === 'Fetch details')"));
  await go("/settings/profile", "[...document.querySelectorAll('button')].some(b => b.textContent === 'Add link')");
  await evalv("[...document.querySelectorAll('button')].find(b => b.textContent === 'Add link').click()");
  await until("!!document.querySelector('input[placeholder=\"Handle or https://\"]')");
  await evalv(`(() => {
    const set = (el, v) => { const proto = el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, v); el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true })); };
    const input = document.querySelector('input[placeholder="Handle or https://"]'); const select = input.closest('.row').querySelector('select');
    set(select, 'twitch'); set(input, '@parity_tester');
  })()`);
  await sleep(200);
  await evalv("[...document.querySelectorAll('button')].find(b => b.textContent === 'Save links').click()");
  await until("document.body.textContent.includes('Links saved.')");
  check("links: handle saved as platform URL", sql(`SELECT url FROM social_links WHERE user_id='${A.id}'`) === "https://twitch.tv/parity_tester");
  await go(`/${A.username}`, "!!document.querySelector('.links a')");
  check("links: platform icon and host shown", (await evalv("(() => { const a = document.querySelector('.links a'); return !!a.querySelector('.platform-icon') && a.querySelector('.link-host').textContent; })()")) === "twitch.tv");

  // Phone: council 2 columns; user card is a bottom sheet with Close.
  await viewport(390, 844);
  await go(`/${A.username}`, "!!document.querySelector('.council')");
  await shot("channel-phone");
  check("council: 2 columns on phone", (await evalv("getComputedStyle(document.querySelector('.council')).gridTemplateColumns.split(' ').length")) === 2);
  await go(`/${A.username}/followers`, "!!document.querySelector('.chip-button')");
  await evalv("document.querySelector('.chip-button').click()");
  await until("!!document.querySelector('.card-head')");
  const sheet = await evalv("(() => { const c = document.querySelector('.user-card'); const s = getComputedStyle(c); const r = c.getBoundingClientRect(); return s.position + '|' + Math.round(innerHeight - r.bottom) + '|' + Math.round(r.width); })()");
  await shot("card-phone");
  check("user card: bottom sheet on phone", sheet === "fixed|0|390", sheet);
  await evalv("document.querySelector('.card-close').click()");
  check("user card: Close button closes", await until("!document.querySelector('.user-card')", 8));
  await viewport(1440, 900);
  await go(`/${A.username}/followers`, "!!document.querySelector('.chip-button')");
  await evalv("document.querySelector('.chip-button').click()");
  await until("!!document.querySelector('.card-head')");
  check("user card: popover on desktop", (await evalv("getComputedStyle(document.querySelector('.user-card')).position + '|' + getComputedStyle(document.querySelector('.card-close')).display")) === "absolute|none");
  // ---------------------------------------------------------------- Parity additions P1-P9
  const click = text => evalv(`(() => { const b = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)}); if (b) b.click(); return !!b; })()`);
  const setValue = (selector, v) => evalv(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(v)}); el.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
  // P9 header copy: defaults on the channel, then owner edits in Studio.
  await go(`/${A.username}`, "!!document.querySelector('.identity')");
  check("P9 header: default label and welcome", (await evalv("document.querySelector('.page-label')?.textContent + '|' + document.querySelector('.welcome-line')?.textContent")) === "Creator Page|Welcome to my page");
  check("P9 header: no intro card or vibe by default", await evalv("!document.querySelector('.intro-card') && !document.querySelector('.page-vibe')"));
  check("P9 header: label has no gradient, glow or radius", await evalv("(() => { const s = getComputedStyle(document.querySelector('.page-label')); return s.backgroundImage === 'none' && s.boxShadow === 'none' && s.borderRadius === '0px' && s.textShadow === 'none'; })()"));
  await go("/studio/channel/header", "!!document.querySelector('input[name=page_label]')");
  await setValue("input[name=page_label]", "Test Label");
  await setValue("textarea[name=intro_body]", "Intro line one\nIntro line two");
  await setValue("input[name=page_vibe]", "Chill nights");
  await click("Save page header");
  await until("document.body.textContent.includes('Page header saved.')");
  await go(`/${A.username}`, "!!document.querySelector('.identity')");
  check("P9 header: edits render on the channel", (await evalv("document.querySelector('.page-label')?.textContent + '|' + document.querySelector('.intro-card h2')?.textContent + '|' + document.querySelector('.intro-body')?.textContent + '|' + document.querySelector('.page-vibe')?.textContent")) === "Test Label|About this page|Intro line one\nIntro line two|Vibe: Chill nights");
  await shot("header-desktop");
  // P3 readiness: banner on Studio channel pages, checklist on the overview, dismiss and restore.
  check("P3 readiness: never on the channel page", await evalv("!document.querySelector('.readiness-banner')"));
  await go("/studio/channel/song", "!!document.querySelector('.readiness-banner')");
  check("P3 readiness: banner shows the count", /Page readiness: \d of 7 done/.test(await evalv("document.querySelector('.readiness-banner').textContent")));
  await go("/studio/channel", "document.querySelectorAll('.readiness-list li').length === 7");
  check("P3 readiness: overview lists 7 steps with done state", (await evalv("document.querySelectorAll('.readiness-list li').length + '|' + document.querySelector('[data-step=song]').dataset.done + '|' + document.querySelector('[data-step=avatar]').dataset.done")) === "7|true|false");
  await go("/studio/channel/song", "!!document.querySelector('.readiness-banner')");
  await click("Dismiss");
  await until("!document.querySelector('.readiness-banner')");
  check("P3 readiness: dismiss stored", sql(`SELECT readiness_dismissed_at IS NOT NULL FROM profiles WHERE user_id='${A.id}'`) === "t");
  await go("/studio/channel/war-council", "document.readyState === 'complete'"); await sleep(800);
  check("P3 readiness: banner stays hidden after dismiss", await evalv("!document.querySelector('.readiness-banner')"));
  await go("/studio/channel", "[...document.querySelectorAll('button')].some(b => b.textContent === 'Show the reminder again')");
  await click("Show the reminder again");
  await until("[...document.querySelectorAll('button')].some(b => b.textContent === 'Hide the reminder')");
  check("P3 readiness: restore clears the dismissal", sql(`SELECT readiness_dismissed_at IS NULL FROM profiles WHERE user_id='${A.id}'`) === "t");
  // P4 setup title, description and a photo through the Studio file input.
  await go("/studio/channel/setup", "!!document.querySelector('input[type=file]')");
  await setValue("input[placeholder='My battle station']", "Battle Station");
  await setValue("textarea", "Desk notes");
  await click("Save setup");
  await until("document.body.textContent.includes('Setup saved.')");
  check("P4 setup: title and description saved", sql(`SELECT setup_title||'|'||setup_description FROM profiles WHERE user_id='${A.id}'`) === "Battle Station|Desk notes");
  const fileInput = (await cmd("Runtime.evaluate", { expression: "document.querySelector('input[type=file]')" }))?.result?.objectId;
  await cmd("DOM.setFileInputFiles", { files: [photoPath], objectId: fileInput });
  check("P4 setup: photo uploads from Studio", await until("!!document.querySelector('.setup-photo-editor img')", 40));
  await go(`/${A.username}/about`, "!!document.querySelector('.setup-photos img')");
  check("P4 setup: About shows title, description and photo", (await evalv("document.querySelector('.setup-section h2').textContent + '|' + document.querySelector('.setup-description').textContent + '|' + /\\/setup\\/[0-9a-f]+\\/400\\.webp$/.test(document.querySelector('.setup-photos img').src) + '|' + /1600\\.webp$/.test(document.querySelector('.setup-photos a').href)")) === "Battle Station|Desk notes|true|true");
  check("P4 setup: owner sees no Report on own photo", await evalv("!document.querySelector('.setup-photos .link-button')"));
  await shot("about-setup");

  // Setup parts picker (docs/PROFILES.md, "Setup parts picker"): search, keyboard pick, custom entry, save, About grouping,
  // then the staff queue approves the custom entry into the list.
  const press = async (key, code, vk) => { for (const type of ["keyDown", "keyUp"]) await cmd("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: vk }); };
  const typeIn = async (category, text) => { await evalv(`(() => { const i = document.querySelector('[data-category=${category}] input[role=combobox]'); i.focus(); i.select(); })()`); await cmd("Input.insertText", { text }); };
  const optionTexts = category => evalv(`JSON.stringify([...document.querySelectorAll('[data-category=${category}] [role=option]')].map(o => o.textContent))`);
  const customName = `Zeta Cam ${process.pid}`;
  await go("/studio/channel/setup", "!!document.querySelector('[data-category=GPU] input[role=combobox]')");
  const blocks = await evalv("JSON.stringify([...document.querySelectorAll('.gear-category')].map(d => d.dataset.category))");
  check("parts: a picker block per category, Other last for kept entries", blocks === JSON.stringify(["CPU", "GPU", "RAM", "MOTHERBOARD", "CAMERA", "MIC", "PERIPHERALS", "OTHER"]), blocks);
  check("parts: no picker in Other, kept names read-only", await evalv("!document.querySelector('[data-category=OTHER] input[role=combobox]') && !document.querySelector('[data-category=OTHER] .gear-name input') && document.querySelector('[data-category=OTHER] .gear-name').textContent === 'Old Key Light'"));
  await typeIn("GPU", "rtx 4070");
  const gpuOpts = await until("(() => { const o = [...document.querySelectorAll('[data-category=GPU] [role=option]')]; return o.length > 1 && o[0].textContent.includes('4070') && JSON.stringify(o.map(x => x.textContent)); })()");
  check("parts: search-as-you-type lists SVER parts, best match first", gpuOpts && JSON.parse(gpuOpts)[0] === "NVIDIA GeForce RTX 4070" && JSON.parse(gpuOpts).every(t => /4070/.test(t) || /custom entry/.test(t)), gpuOpts);
  check("parts: combobox expanded with a listbox", (await evalv("document.querySelector('[data-category=GPU] input[role=combobox]').getAttribute('aria-expanded')")) === "true");
  await press("ArrowDown", "ArrowDown", 40);
  check("parts: arrow key sets the active option", (await evalv("(() => { const i = document.querySelector('[data-category=GPU] input[role=combobox]'); return document.getElementById(i.getAttribute('aria-activedescendant'))?.getAttribute('aria-selected'); })()")) === "true");
  await press("Enter", "Enter", 13);
  check("parts: Enter picks the part as an SVER list item", !!(await until("[...document.querySelectorAll('[data-category=GPU] .gear-name')].some(n => n.textContent === 'NVIDIA GeForce RTX 4070 SVER list')", 20)));
  check("parts: picker clears after a pick", (await evalv("document.querySelector('[data-category=GPU] input[role=combobox]').value")) === "");
  await typeIn("MIC", "shure");
  await until("document.querySelectorAll('[data-category=MIC] [role=option]').length > 1", 20);
  await press("Escape", "Escape", 27);
  check("parts: Escape closes the list", (await evalv("document.querySelector('[data-category=MIC] input[role=combobox]').getAttribute('aria-expanded')")) === "false");
  await evalv("document.querySelector('[data-category=MIC] input[role=combobox]').blur()");
  // Mic also lists audio interfaces and mixers (Joe's decision, 5:52 PM ET), marked as such.
  await typeIn("MIC", "goxlr mini");
  const micOpt = await until("(() => { const o = document.querySelector('[data-category=MIC] [role=option]'); return o && o.textContent.includes('GoXLR Mini') && o.textContent; })()", 20);
  check("parts: Mic search lists interfaces, labeled", micOpt === "TC-Helicon GoXLR Mini · Audio interface", micOpt || (await optionTexts("MIC")) + " value=" + (await evalv("document.querySelector('[data-category=MIC] input[role=combobox]').value")));
  await press("ArrowDown", "ArrowDown", 40); await press("Enter", "Enter", 13);
  check("parts: picked interface tagged Audio interface", !!(await until("[...document.querySelectorAll('[data-category=MIC] .gear-name')].some(n => n.textContent === 'TC-Helicon GoXLR Mini SVER list Audio interface')", 20)));
  await typeIn("CAMERA", customName);
  const camOpts = await until(`(() => { const o = [...document.querySelectorAll('[data-category=CAMERA] [role=option]')]; const last = o[o.length - 1]?.textContent || ''; return last.includes(${JSON.stringify(customName)}) && last; })()`, 20);
  check("parts: unknown name offers a custom entry", /as a custom entry/.test(camOpts || ""), camOpts || await optionTexts("CAMERA"));
  await press("Enter", "Enter", 13);
  check("parts: custom entry added with a Custom tag", !!(await until(`[...document.querySelectorAll('[data-category=CAMERA] .gear-name')].some(n => n.querySelector('input')?.value === ${JSON.stringify(customName)} && n.textContent.includes('Custom'))`, 20)));
  await click("Save setup");
  await until("/Saved/.test(document.body.innerText)", 20);
  check("parts: picked part saved with its part ID", sql(`SELECT count(*) FROM setup_items s JOIN parts p ON p.id=s.part_id WHERE s.user_id='${A.id}' AND s.category='GPU' AND s.name='NVIDIA GeForce RTX 4070'`) === "1");
  check("parts: custom entry saved and queued as pending", sql(`SELECT s.name||'|'||coalesce(s.part_id,'-')||'|'||p.status FROM setup_items s JOIN part_submissions p ON p.id=s.submission_id WHERE s.user_id='${A.id}' AND s.category='CAMERA' AND s.name='${customName}'`) === `${customName}|-|PENDING`);
  check("parts: kept Other entry and its legacy category survive the save", sql(`SELECT category||'|'||legacy_category FROM setup_items WHERE user_id='${A.id}' AND name='Old Key Light'`) === "OTHER|LIGHTING");
  await go("/studio/channel/setup", "!!document.querySelector('[data-category=CAMERA] .gear-name')");
  check("parts: Studio shows the pending review", !!(await until(`[...document.querySelectorAll('[data-category=CAMERA] .gear-name')].some(n => n.textContent.includes('Custom · waiting for review'))`, 20)));
  await shot("studio-parts");
  await go(`/${A.username}/about`, "!!document.querySelector('.setup')");
  const groups2 = await evalv("JSON.stringify([...document.querySelectorAll('.setup > div')].map(d => [d.querySelector('dt').textContent, [...d.querySelectorAll('dd')].map(x => x.textContent.split(' — ')[0])]))");
  check("parts: About groups by picker category, custom entry shown right away", (() => { const g = JSON.parse(groups2 || "[]"); return JSON.stringify(g.map(x => x[0])) === JSON.stringify(["GPU", "Camera", "Mic & audio interface", "Other"]) && g[0][1].some(t => t.includes("NVIDIA GeForce RTX 4070")) && g[1][1].some(t => t.includes(customName)); })(), groups2);
  check("parts: About marks the interface", await evalv("[...document.querySelectorAll('.setup dd')].some(d => d.textContent === 'TC-Helicon GoXLR Mini · Audio interface')"));
  check("parts: non-staff get 404 on the queue", (await evalv("fetch('/admin/parts').then(r => r.status)")) === 404);
  await as(S);
  await go("/admin/parts", `[...document.querySelectorAll('section h2')].some(h => h.textContent === ${JSON.stringify(`Camera · ${customName}`)})`);
  const entry = `[...document.querySelectorAll('section')].find(x => x.querySelector('h2')?.textContent === ${JSON.stringify(`Camera · ${customName}`)})`;
  check("parts queue: entry shows submitter and use count", new RegExp(`First added by @${A.username} .* used in 1 setup$`).test(await evalv(`${entry}.querySelector('p.muted').textContent`) || ""), await evalv(`${entry}.querySelector('p.muted').textContent`));
  check("parts queue: approve form prefilled from the typed name", (await evalv(`${entry}.querySelector('input[name=brand]').value + '|' + ${entry}.querySelector('input[name=model]').value`)) === `Zeta|Cam ${process.pid}`);
  await evalv(`(() => { const f = ${entry}.querySelector('form'); f.querySelector('input[name=brand]').value = 'ZETA'; f.requestSubmit(f.querySelector('button[value=approve]')); })()`);
  const notice = await until("document.querySelector('[role=status]')?.textContent");
  check("parts queue: approve adds it to the list and links the setup", notice === `Added “ZETA Cam ${process.pid}” to the list; 1 setup entry linked.`, notice);
  check("parts queue: submission approved and setup item linked", sql(`SELECT p.status||'|'||s.name||'|'||(s.part_id=p.part_id) FROM part_submissions p JOIN setup_items s ON s.submission_id=p.id WHERE s.user_id='${A.id}' AND s.category='CAMERA' AND p.norm='zeta cam ${process.pid}'`) === `APPROVED|ZETA Cam ${process.pid}|true`);
  check("parts queue: approval audited", sql(`SELECT count(*) FROM moderation_actions WHERE action='part_approved' AND actor_id='${S.id}'`) === "1");
  await click("Added");
  check("parts queue: Added tab lists the decision", !!(await until(`[...document.querySelectorAll('section p')].some(p => p.textContent.startsWith(${JSON.stringify(`Added as “ZETA Cam ${process.pid}” by @${S.username}`)}))`, 20)));
  await shot("admin-parts");
  await as(A);
  await go("/studio/channel/setup", "!!document.querySelector('[data-category=CAMERA] .gear-name')");
  check("parts: approved entry now shows as an SVER list item", !!(await until(`[...document.querySelectorAll('[data-category=CAMERA] .gear-name')].some(n => n.textContent === ${JSON.stringify(`ZETA Cam ${process.pid} SVER list`)})`, 20)));
  // P2 mood presets.
  await go("/settings/profile", "!!document.querySelector('.mood-presets')");
  check("P2 mood: 12 presets plus Clear", (await evalv("document.querySelectorAll('.mood-presets button').length")) === 13);
  await evalv("document.querySelector('.mood-presets button[aria-label=\"Mood 🎮\"]').click()");
  check("P2 mood: preset fills the field and is pressed", (await until("document.querySelector('input[name=mood_emoji]').value === '🎮' && document.querySelector('.mood-presets [aria-pressed=true]')?.textContent")) === "🎮");
  await click("Save identity");
  await until("document.body.textContent.includes('Saved.')");
  check("P2 mood: saved", sql(`SELECT mood_emoji FROM profiles WHERE user_id='${A.id}'`) === "🎮");
  // P5 suggestions: Twitch is already linked as a social link, so only Discord is suggested; Add fills the form only.
  const suggestion = await until("document.querySelector('.suggestions')?.textContent");
  check("P5 links: Discord suggested from the linked account, Twitch suppressed", /discord\.com\/users\/\d+/.test(suggestion || "") && !/pr_twitch_handle/.test(suggestion || ""), suggestion);
  await evalv("[...document.querySelectorAll('.suggestions button')].find(b => b.textContent === 'Add').click()");
  await sleep(200);
  check("P5 links: Add doesn't save by itself", sql(`SELECT count(*) FROM social_links WHERE user_id='${A.id}' AND platform='discord'`) === "0");
  await click("Save links");
  await until("document.body.textContent.includes('Links saved.')");
  check("P5 links: saved after Save links", /^https:\/\/discord\.com\/users\/\d+$/.test(sql(`SELECT url FROM social_links WHERE user_id='${A.id}' AND platform='discord'`)));
  // P6 Also known as: off by default, then opt in.
  check("P6 card: opt-in checkbox off by default", (await evalv("[...document.querySelectorAll('input[type=checkbox]')].find(i => i.parentElement.textContent.includes('linked Twitch and Discord')).checked")) === false);
  await evalv("[...document.querySelectorAll('input[type=checkbox]')].find(i => i.parentElement.textContent.includes('linked Twitch and Discord')).click()");
  await until("document.body.textContent.includes('now show on your user card')");
  check("P6 card: opt-in saved", sql(`SELECT show_linked_accounts FROM profiles WHERE user_id='${A.id}'`) === "t");
  // P1 share as the owner: copies the canonical URL from a sub-tab.
  await go(`/${A.username}/wall`, "!!document.querySelector('.share button')");
  await evalv("document.querySelector('.share button').click()");
  check("P1 share: copies the canonical channel URL", (await until("window.__copied.join()")) === `${origin}/${A.username}`);
  check("P1 share: confirms with Link copied", !!(await until("document.querySelector('.share button').textContent === 'Link copied'", 8)));
  await evalv("window.__clipFail = true");
  await go(`/${A.username}/wall`, "!!document.querySelector('.share button')");
  await evalv("window.__clipFail = true; document.querySelector('.share button').click()");
  check("P1 share: fallback field when the clipboard fails", (await until("document.querySelector('.share-fallback input')?.value")) === `${origin}/${A.username}`);
  await go(`/${A.username}`, "!!document.querySelector('.share button')");
  await evalv("window.matchMedia = q => ({ matches: q.includes('coarse') }); navigator.share = async d => { window.__shared = d; }; document.querySelector('.share button').click()");
  check("P1 share: native share sheet on touch devices", (await until("window.__shared && window.__shared.url")) === `${origin}/${A.username}`);
  // P8 rewards stub.
  await go(`/${A.username}/rewards`, "!!document.querySelector('.channel')");
  check("P8 rewards: placeholder renders", await evalv("!!document.querySelector('[data-stub=rewards]') && document.body.textContent.includes('Rewards are coming')"));
  check("P8 rewards: no Rewards tab", await evalv("![...document.querySelectorAll('.channel-tabs a')].some(a => a.textContent === 'Rewards')"));
  check("P8 rewards: 404 for an unknown channel", (await fetch(`${origin}/no_such_user_${A.id.slice(0, 6)}/rewards`)).status === 404);

  // Viewer B: share, report on setup photos, the opt-in card and B's own activity feed.
  await as(B);
  await go(`/${A.username}/about`, "!!document.querySelector('.setup-photos img')");
  check("P4 setup: viewers get Report on photos", await evalv("[...document.querySelectorAll('.setup-photos .link-button')].some(b => b.textContent === 'Report')"));
  await go(`/${B.username}/following`, "!!document.querySelector('.chip-button')");
  await evalv("document.querySelector('.chip-button').click()");
  const aka = await until("document.querySelector('.card-aka')?.textContent");
  check("P6 card: Also known as shows Twitch and Discord", /Also known as/.test(aka || "") && /pr_twitch_handle/.test(aka || "") && /pr\.discord/.test(aka || ""), aka);
  check("P6 card: Twitch links out, Discord is text", (await evalv("[...document.querySelectorAll('.card-aka a')].map(a => a.getAttribute('href') + ' ' + a.getAttribute('rel')).join()")) === "https://twitch.tv/pr_twitch_handle nofollow noopener noreferrer");
  // B unfollowed and refollowed A in the browser earlier, which recorded one follow event.
  for (let i = 0; i < 7; i++) sql(`INSERT INTO activity_events(id,actor_id,kind,data,created_at) VALUES('${randomUUID()}','${B.id}','schedule','{}',now()-interval '2 days'-make_interval(mins=>${i}))`);
  await go(`/${B.username}`, "!!document.querySelector('.activity-list')");
  check("P7 activity: follow event shown first", (await evalv("document.querySelector('.activity-list li').dataset.kind + '|' + document.querySelector('.activity-list li').textContent.includes('Followed')")) === "follow|true");
  check("P7 activity: 5 shown before Show more", (await evalv("document.querySelectorAll('.activity-list li').length")) === 5);
  await click("Show more");
  check("P7 activity: Show more reveals the rest", !!(await until("document.querySelectorAll('.activity-list li').length === 8")));
  await shot("activity");
  await cmd("Network.deleteCookies", { name: "sver_dev", url: origin });
  await go(`/${A.username}`, "!!document.querySelector('.identity')");
  check("P1 share: shown to signed-out visitors", await evalv("!!document.querySelector('.share button')"));
  check("no uncaught page exceptions", exceptions.length === 0, exceptions.join(" || "));
  sock.close();
} finally {
  chrome.kill();
  try { (await import("node:fs")).unlinkSync(photoPath); } catch {}
  sql(`DELETE FROM sessions WHERE user_id IN ('${A.id}','${B.id}','${S.id}'); DELETE FROM users WHERE (id='${A.id}' AND username='${A.username}') OR (id='${B.id}' AND username='${B.username}') OR (id='${S.id}' AND username='${S.username}'); DELETE FROM parts WHERE source='STAFF' AND norm='zeta cam ${process.pid}';`);
  assert.equal(sql(`SELECT count(*) FROM users WHERE id IN ('${A.id}','${B.id}','${S.id}')`), "0");
}
for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"} ${r.name}${r.ok || !r.detail ? "" : `  (${r.detail})`}`);
const failed = results.filter(r => !r.ok).length;
console.log(`${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
