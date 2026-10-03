// Isolated real-media protocol proof. No production endpoints or account database.
// Requires Docker, ffmpeg, and optionally MEDIA_PROOF_PYTHON with aiortc 1.14.0.
const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const http = require('node:http');
const assert = require('node:assert/strict');

if (process.argv[2] === '--hooks') {
  // Controlled authorization fixture, deliberately not the production auth service.
  const config = JSON.parse(fs.readFileSync('/fixture/credentials.json'));
  const state = { accepted: 0, rejected: 0, unpublished: 0, active: null, enabled: true, instance: null };
  http.createServer(async (req, res) => {
    let body = '';
    for await (const chunk of req) {
      body += chunk;
      if (body.length > 8192) { res.writeHead(413).end(); return; }
    }
    res.setHeader('Content-Type', 'application/json');
    try {
      if (req.url === '/stats' && req.method === 'GET') {
        res.end(JSON.stringify(state)); return;
      }
      if (req.url === '/control' && req.headers.authorization === `Bearer ${config.control}`) {
        const input = JSON.parse(body);
        if (typeof input.enabled === 'boolean') state.enabled = input.enabled;
        if (typeof input.key === 'string') config.key = input.key;
        res.end('{}'); return;
      }
      if (req.url !== `/hook/${config.hook}` || req.method !== 'POST') {
        res.writeHead(403).end('{"code":403}'); return;
      }
      const event = JSON.parse(body);
      if (event.action === 'on_publish') {
        const key = new URLSearchParams(event.param).getAll('key');
        const allowed = state.enabled && event.app === 'live' && event.stream === 'channel'
          && key.length === 1 && key[0] === config.key;
        if (allowed) {
          state.accepted++; state.active = event.client_id;
          state.instance = { server: event.server_id, service: event.service_id };
        }
        else state.rejected++;
        res.end(JSON.stringify({ code: allowed ? 0 : 403 }));
      } else if (event.action === 'on_unpublish') {
        state.unpublished++;
        if (state.active === event.client_id) state.active = null;
        res.end('{"code":0}');
      } else res.writeHead(403).end('{"code":403}');
    } catch { res.writeHead(400).end('{"code":400}'); }
  }).listen(8089, '0.0.0.0');
}

const SRS = 'ossrs/srs@sha256:2e96f38660211b8e8dd324bee0d3ade90f1e44ab815813a1684eb78ab2ad17d6';
const LLHLS_REFERENCE = 'sha256:6f58c6dc101a6a62fb01ae81ba731f45f9d3fbfd5110942e078466959ec81361';
let interrupted = false;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function docker(args) {
  try { return execFileSync('docker', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 45000 }).trim(); }
  catch { throw Error(`Docker ${args[0]} failed${args[0] === 'port' ? ` for ${args[1]}:${args[2]}` : ''} (raw output suppressed to protect fixture credentials)`); }
}
async function until(check, label, timeout = 25000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (interrupted) throw Error('Interrupted; cleaning up fixture');
    try { const value = await check(); if (value) return value; } catch {}
    await sleep(150);
  }
  throw Error(`Timed out: ${label}`);
}
async function get(url, options = {}) {
  const response = await fetch(url, { ...options, signal: AbortSignal.timeout(5000) });
  if (!response.ok) throw Error(`HTTP ${response.status} on fixture request`);
  return response;
}
function hostPort(name, port) {
  const binding = docker(['port', name, `${port}/tcp`]);
  assert.match(binding, /^127\.0\.0\.1:\d+$/);
  return Number(binding.split(':')[1]);
}
function startEncoder(url, children) {
  const child = spawn(process.env.FFMPEG || 'ffmpeg', [
    '-hide_banner', '-loglevel', 'error', '-f', 'rawvideo', '-pixel_format', 'rgb24',
    '-video_size', '320x180', '-framerate', '10', '-i', 'pipe:0',
    '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000',
    '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-pix_fmt', 'yuv420p',
    '-profile:v', 'baseline', '-bf', '0', '-g', '10', '-b:v', '400k',
    '-c:a', 'aac', '-b:a', '64k', '-t', '120', '-f', 'flv', url,
  ], { stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true });
  children.push(child);
  let failed = false;
  child.on('error', () => { failed = true; });
  child.stderr.on('data', () => {}); // FFmpeg errors can contain the publishing secret.
  child.stdin.on('error', () => {});
  // Encode a machine-readable capture clock into the synthetic pixels. It measures
  // capture-to-decode, not browser presentation or glass-to-glass latency.
  const timer = setInterval(() => {
    if (failed || child.exitCode !== null || child.stdin.destroyed || child.stdin.writableNeedDrain) return;
    const frame = Buffer.alloc(320 * 180 * 3, 80);
    const stamp = Math.floor(Date.now() / 10) >>> 0;
    for (let bit = 0; bit < 32; bit++) {
      const value = (stamp >>> bit) & 1 ? 235 : 16;
      for (let y = 20; y < 60; y++) frame.fill(value, (y * 320 + bit * 10) * 3, (y * 320 + bit * 10 + 10) * 3);
    }
    child.stdin.write(frame);
  }, 100);
  child.on('exit', () => clearInterval(timer));
  child.stop = async () => {
    clearInterval(timer);
    if (child.exitCode === null) {
      child.stdin.destroy();
      child.kill();
      await until(() => child.exitCode !== null || child.signalCode !== null, 'encoder stop', 6000);
    }
  };
  return child;
}
function decodeClock(frame) {
  let stamp = 0;
  for (let bit = 0; bit < 32; bit++) {
    const pixel = (40 * 320 + bit * 10 + 5) * 3;
    if (frame[pixel] > 128) stamp = (stamp | (1 << bit)) >>> 0;
  }
  const now = Math.floor(Date.now() / 10) >>> 0;
  return ((now - stamp) >>> 0) * 10;
}
async function decodeHls(url, children, parts = false) {
  const child = spawn(process.env.FFMPEG || 'ffmpeg', [
    '-hide_banner', '-loglevel', 'error',
    ...(parts ? ['-probesize', '32', '-analyzeduration', '0', '-i', 'pipe:0'] : ['-live_start_index', '-2', '-i', url]),
    '-map', '0:v:0', '-frames:v', '40', '-pix_fmt', 'rgb24', '-f', 'rawvideo', 'pipe:1',
  ], { stdio: [parts ? 'pipe' : 'ignore', 'pipe', 'pipe'], windowsHide: true });
  children.push(child);
  let data = Buffer.alloc(0);
  const delays = [];
  child.stdout.on('data', chunk => {
    data = Buffer.concat([data, chunk]);
    while (data.length >= 320 * 180 * 3) {
      delays.push(decodeClock(data));
      data = data.subarray(320 * 180 * 3);
    }
  });
  child.stderr.on('data', () => {});
  if (parts) {
    child.stdin.on('error', () => {}); // Decoder closes input after its frame limit.
    let initialized = false;
    const seen = new Set();
    const started = Date.now();
    while (child.exitCode === null && Date.now() - started < 30000) {
      if (interrupted) throw Error('Interrupted; cleaning up fixture');
      const playlist = await (await get(url)).text();
      let entries = [...playlist.matchAll(/#EXT-X-PART:([^\n]*URI="([^"]+)"[^\n]*)/g)];
      const write = async name => {
        const target = new URL(name, url);
        assert.equal(target.origin, new URL(url).origin);
        const bytes = Buffer.from(await (await get(target)).arrayBuffer());
        if (!child.stdin.destroyed) child.stdin.write(bytes);
      };
      if (!initialized) {
        const independent = entries.findLastIndex(entry => entry[1].includes('INDEPENDENT=YES'));
        if (independent === -1) { await sleep(150); continue; }
        entries.slice(0, independent).forEach(entry => seen.add(entry[2]));
        entries = entries.slice(independent);
        const init = playlist.match(/#EXT-X-MAP:URI="([^"]+)"/);
        assert(init, 'Missing LL-HLS initialization segment');
        await write(init[1]);
        initialized = true;
      }
      for (const entry of entries) {
        if (seen.has(entry[2]) || child.exitCode !== null) continue;
        await write(entry[2]);
        seen.add(entry[2]);
      }
      await sleep(100);
    }
  }
  await until(() => child.exitCode !== null, 'HLS video decode', parts ? 2000 : 30000);
  assert.equal(child.exitCode, 0, 'HLS decoder exited unsuccessfully');
  assert.equal(delays.length, 40);
  assert(delays.every(ms => ms >= 0 && ms < 30000), 'Capture clock did not survive H.264');
  delays.sort((a, b) => a - b);
  return { decoded_video_frames: 40, capture_to_decode_p50_ms: delays[19], capture_to_decode_p95_ms: delays[37] };
}

async function main() {
  process.once('SIGINT', () => { interrupted = true; });
  process.once('SIGTERM', () => { interrupted = true; });
  // Refuse a remote Docker daemon; the test binds only local disposable media ports.
  if (process.env.DOCKER_HOST && !/^(npipe:|unix:)/.test(process.env.DOCKER_HOST)) throw Error('Use a local Docker daemon');
  const context = JSON.parse(docker(['context', 'inspect']))[0];
  assert.match(context.Endpoints.docker.Host, /^(npipe:|unix:)/, 'Docker context must be local');
  docker(['image', 'inspect', SRS, '--format', '{{.Id}}']);
  docker(['image', 'inspect', 'node:22-alpine', '--format', '{{.Id}}']);
  const reference = process.argv.includes('--llhls-reference');
  if (reference) {
    docker(['image', 'inspect', LLHLS_REFERENCE, '--format', '{{.Id}}']);
    docker(['image', 'inspect', 'nginx:1.28-alpine', '--format', '{{.Id}}']);
  }
  const environment = { platform: process.platform, node: process.version,
    ffmpeg: execFileSync(process.env.FFMPEG || 'ffmpeg', ['-version'], { encoding: 'utf8', windowsHide: true, timeout: 5000 }).split(/\r?\n/)[0] };
  if (process.env.MEDIA_PROOF_PYTHON) {
    environment.aiortc = execFileSync(process.env.MEDIA_PROOF_PYTHON, ['-c', 'import aiortc; print(aiortc.__version__)'], { encoding: 'utf8', windowsHide: true, timeout: 10000 }).trim();
    assert.equal(environment.aiortc, '1.14.0', 'Use the pinned test-only aiortc version');
  }
  const run = `sver-media-proof-${crypto.randomBytes(5).toString('hex')}`;
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), `${run}-`));
  const credentials = { key: crypto.randomBytes(32).toString('hex'), hook: crypto.randomBytes(32).toString('hex'), control: crypto.randomBytes(32).toString('hex') };
  const children = [], containers = [];
  let network = false;
  const result = {
    checked_at: new Date().toISOString(), scope: 'local real media with controlled authorization fixture',
    srs_image: SRS, passed: false, latency_acceptance: false,
    environment,
    input: { width: 320, height: 180, fps: 10, video: 'H.264 baseline', video_kbps: 400, keyframe_seconds: 1, b_frames: 0, audio: 'AAC 48 kHz', audio_kbps: 64 },
    measurement: '40 video frames per path; local capture-to-decode; no render, network-region or sustained-load acceptance',
  };
  try {
    fs.writeFileSync(path.join(directory, 'credentials.json'), JSON.stringify(credentials));
    fs.copyFileSync(__filename, path.join(directory, 'check-media.cjs'));
    fs.writeFileSync(path.join(directory, 'srs.conf'), `listen 1935;
daemon off;
srs_log_tank console;
srs_log_level error;
rtc_server { enabled on; listen 18000; candidate 127.0.0.1; }
http_api { enabled on; listen 1985; }
http_server { enabled on; listen 8080; dir /media; }
vhost __defaultVhost__ {
  rtc { enabled on; rtmp_to_rtc on; }
  play { gop_cache off; }
  hls { enabled on; hls_path /media; hls_ctx off; hls_fragment 1; hls_window 6; hls_wait_keyframe on; hls_ts_file [app]/[stream]-[timestamp]-[seq].ts; }
  http_hooks { enabled on; on_publish http://hooks:8089/hook/${credentials.hook}; on_unpublish http://hooks:8089/hook/${credentials.hook}; }
}
`);
    // Docker Desktop does not publish ports for an --internal bridge. Use a
    // dedicated bridge with every published port explicitly bound to loopback.
    docker(['network', 'create', '--label', `sver.media-proof=${run}`, run]); network = true;
    const mount = `${directory.replaceAll('\\', '/')}:/fixture:ro`;
    const hooks = `${run}-hooks`, srs = `${run}-srs`;
    for (const [name, args] of [
      [hooks, ['--network-alias', 'hooks', '-p', '127.0.0.1::8089', '-v', mount, 'node:22-alpine', 'node', '/fixture/check-media.cjs', '--hooks']],
      [srs, ['-p', '127.0.0.1::1935', '-p', '127.0.0.1::1985', '-p', '127.0.0.1::8080', '-p', '127.0.0.1::8088', '-p', '127.0.0.1:18000:18000/udp', '-v', mount, '--tmpfs', '/media:size=128m', SRS, './objs/srs', '-c', '/fixture/srs.conf']],
    ]) {
      containers.push(name);
      docker(['run', '-d', '--pull=never', '--name', name, '--network', run, '--label', `sver.media-proof=${run}`, '--cpus=1', '--memory=512m', ...args]);
    }
    if (reference) {
      // Start containers before feeding video: synchronous Docker startup can
      // otherwise stall the capture timer long enough for SRS to drop ingest.
      // This is a read-only artifact comparison, never a runtime dependency.
      fs.writeFileSync(path.join(directory, 'nginx.conf'), `events {}\nhttp { access_log off; server { listen 8088; location / { proxy_pass http://127.0.0.1:8086; proxy_buffering off; proxy_read_timeout 10s; } } }`);
      for (const [name, args] of [
        [`${run}-reference`, [LLHLS_REFERENCE]],
        [`${run}-proxy`, ['-v', `${directory.replaceAll('\\', '/')}/nginx.conf:/etc/nginx/nginx.conf:ro`, 'nginx:1.28-alpine']],
      ]) {
        containers.push(name);
        docker(['run', '-d', '--pull=never', '--name', name, '--network', `container:${srs}`, '--label', `sver.media-proof=${run}`, '--memory=512m', '--cpus=1', ...args]);
      }
    }
    const control = `http://127.0.0.1:${hostPort(hooks, 8089)}`;
    const api = `http://127.0.0.1:${hostPort(srs, 1985)}`;
    const origin = `http://127.0.0.1:${hostPort(srs, 8080)}`;
    const rtmp = `rtmp://127.0.0.1:${hostPort(srs, 1935)}/live/channel`;
    const referenceOrigin = reference ? `http://127.0.0.1:${hostPort(srs, 8088)}` : undefined;
    const version = await (await until(() => get(`${api}/api/v1/versions`), 'SRS ready')).json();
    result.srs_version = version.data.version;
    assert.match(version.server, /^[A-Za-z0-9_-]{1,64}$/);
    assert.match(version.service, /^[A-Za-z0-9_-]{1,64}$/);
    const stats = async () => (await get(`${control}/stats`)).json();
    const configure = body => get(`${control}/control`, { method: 'POST', headers: { authorization: `Bearer ${credentials.control}` }, body: JSON.stringify(body) });
    const active = async () => ((await (await get(`${api}/api/v1/streams`)).json()).streams || []).find(stream => stream.publish?.active && stream.name === 'channel');
    const reject = async url => {
      const encoder = startEncoder(url, children);
      await until(() => encoder.exitCode !== null, 'publish rejected');
      assert.notEqual(encoder.exitCode, 0, 'Rejected publisher unexpectedly succeeded');
      assert.equal(await active(), undefined);
    };
    assert.equal((await fetch(`${control}/hook/wrong`, { method: 'POST', body: '{}' })).status, 403);
    await reject(rtmp);
    await reject(`${rtmp}?key=wrong`);
    await reject(`${rtmp}?key=${credentials.key}&key=${credentials.key}`);
    result.missing_wrong_duplicate_credentials_rejected = true;
    console.log('PASS: SRS forwards a separate publishing parameter; invalid credentials rejected');

    const encoder = startEncoder(`${rtmp}?key=${credentials.key}`, children);
    const first = await until(active, 'valid publish');
    assert.deepEqual((await stats()).instance, { server: version.server, service: version.service });
    const streamInventory = await (await get(`${api}/api/v1/streams`)).json();
    assert.equal(streamInventory.server, version.server);
    assert.equal(streamInventory.service, version.service);
    result.callback_boot_identity = true;
    const manifest = await until(async () => {
      const text = await (await get(`${origin}/live/channel.m3u8`)).text();
      return text.includes('#EXTINF:') && text;
    }, 'HLS manifest');
    assert(!manifest.includes(credentials.key), 'Publishing secret leaked into manifest');
    for (const line of manifest.split('\n').filter(line => line && !line.startsWith('#'))) {
      assert.match(line.trim(), /^channel-[\d-]+\.ts$/);
      assert((await (await get(new URL(line.trim(), `${origin}/live/channel.m3u8`))).arrayBuffer()).byteLength > 0);
    }
    result.public_playback_has_no_publish_secret = true;
    result.hls = await decodeHls(`${origin}/live/channel.m3u8`, children);
    console.log('PASS: real H.264 HLS frames decoded with intact capture timestamps');

    const duplicate = startEncoder(`${rtmp}?key=${credentials.key}`, children);
    await until(() => duplicate.exitCode !== null, 'second publisher rejected');
    assert.notEqual(duplicate.exitCode, 0);
    assert.equal((await active()).publish.cid, first.publish.cid);
    result.single_publisher_preserved = true;

    if (process.env.MEDIA_PROOF_PYTHON) {
      const receiver = spawn(process.env.MEDIA_PROOF_PYTHON, [path.join(__dirname, 'media-webrtc.py'), `${api}/rtc/v1/whep/?app=live&stream=channel`], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
      children.push(receiver);
      let receipt = '', receiverError = '';
      receiver.stdout.on('data', bytes => { receipt += bytes; });
      receiver.stderr.on('data', bytes => { receiverError = (receiverError + bytes).slice(-1500); });
      await until(() => receiver.exitCode !== null, 'WebRTC decode', 30000);
      assert.equal(receiver.exitCode, 0, `WebRTC receiver failed: ${receiverError.split('\n').slice(-4).join(' ').replace(/[a-f0-9]{64}/g, '[redacted]')}`);
      result.webrtc = JSON.parse(receipt);
      console.log('PASS: real WebRTC audio/video decoded');
    } else result.webrtc = { skipped: 'Set MEDIA_PROOF_PYTHON to an isolated Python environment with aiortc==1.14.0' };

    let llhls;
    if (reference) {
      llhls = `${referenceOrigin}/channel.${first.publish.cid}`;
      const playlist = await until(async () => {
        const text = await (await get(`${llhls}/media.m3u8`)).text();
        return text.includes('#EXT-X-PART:') && text;
      }, 'reference LL-HLS partial segments', 40000);
      assert(!playlist.includes(credentials.key));
      const part = playlist.match(/#EXT-X-PART:[^\n]*URI="([^"]+)"/)[1];
      assert((await (await get(new URL(part, `${llhls}/media.m3u8`))).arrayBuffer()).byteLength > 0);
      const hint = playlist.match(/#EXT-X-PRELOAD-HINT:[^\n]*URI="part-\d+-(\d+)\.(\d+)\.m4s"/);
      assert(hint, 'No LL-HLS preload hint');
      const started = Date.now();
      const reloaded = await (await get(`${llhls}/media.m3u8?_HLS_msn=${hint[1]}&_HLS_part=${hint[2]}`)).text();
      assert(reloaded.includes('#EXT-X-PART:'));
      assert.equal((await fetch(`${llhls}/media.m3u8?_HLS_part=0`)).status, 400);
      const blockingMs = Date.now() - started;
      result.llhls_reference = { image: LLHLS_REFERENCE, partial_segment_retrieved: true, blocking_reload_ms: blockingMs, malformed_reload_rejected: true,
        full_segment_decode: await decodeHls(`${llhls}/master.m3u8`, children),
        partial_segment_decode: await decodeHls(`${llhls}/media.m3u8`, children, true) };
      console.log('PASS: legacy reference LL-HLS full segments and live-edge parts decode');
    }
    await encoder.stop();
    await until(async () => !(await active()), 'disconnect');
    const again = startEncoder(`${rtmp}?key=${credentials.key}`, children);
    const second = await until(active, 'reconnect');
    assert.notEqual(second.publish.cid, first.publish.cid);
    if (llhls) {
      await until(async () => (await fetch(`${llhls}/media.m3u8`)).status === 404, 'old reference media removed');
      result.llhls_reference.old_publisher_route_removed = true;
    }
    result.reconnect_changes_media_client = true;
    await configure({ enabled: false });
    await get(`${api}/api/v1/clients/${second.publish.cid}`, { method: 'DELETE' });
    await until(async () => !(await active()), 'publisher kick');
    await again.stop();
    await reject(`${rtmp}?key=${credentials.key}`);
    result.kick_and_revocation_rejected_republish = true;
    await configure({ key: crypto.randomBytes(32).toString('hex'), enabled: true });
    await reject(`${rtmp}?key=${credentials.key}`);
    result.rotated_key_rejected = true;
    await configure({ key: credentials.key });
    docker(['stop', '--time', '2', hooks]);
    await reject(`${rtmp}?key=${credentials.key}`);
    result.hook_outage_fails_closed = true;
    result.passed = true;
    result.not_established = ['production auth integration', '60-second persisted broadcast lifecycle', 'OBS UI', 'browser render latency', 'Bunny CDN', 'rebuild LL-HLS implementation', 'capacity threshold'];
  } finally {
    for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill();
    for (const name of containers.reverse()) {
      if (!docker(['ps', '-aq', '--filter', `name=^${name}$`, '--filter', `label=sver.media-proof=${run}`])) continue;
      assert.equal(docker(['inspect', '--format', '{{index .Config.Labels "sver.media-proof"}}', name]), run);
      if (!result.passed) {
        const logs = docker(['logs', '--tail', '8', name]).replace(/[a-f0-9]{64}/g, '[fixture-secret-redacted]');
        if (logs) console.error(`${name}: ${logs}`);
      }
      docker(['rm', '-f', name]);
    }
    if (network) docker(['network', 'rm', run]);
    // Delete only the files we created; no recursive or computed-tree deletion.
    for (const name of ['credentials.json', 'check-media.cjs', 'srs.conf', 'nginx.conf']) {
      const file = path.join(directory, name);
      if (fs.existsSync(file)) fs.unlinkSync(file);
    }
    fs.rmdirSync(directory);
  }
  const destination = path.resolve(__dirname, '../docs/media-proof-local.json');
  fs.writeFileSync(destination, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result, null, 2));
}

if (process.argv[2] !== '--hooks') {
  main().catch(error => { console.error(`Media proof FAILED: ${error.message}`); process.exitCode = 1; });
}
