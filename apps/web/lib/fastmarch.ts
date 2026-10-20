/** Fastmarch (docs/FASTMARCH.md): couriers racing through MAGNet. Pure and deterministic from a seed,
 *  so a later step can replay a run on the server. Rider 0 is the player; the rest are computer couriers. */

export type Dir = 0 | 1 | 2 | 3; // up, right, down, left
export type Rider = { x: number; y: number; dir: Dir; alive: boolean; diedAt: number | null };
/** cells: 0 open, 1 the Static, 2 + i rider i's trail. */
export type Game = { w: number; h: number; cells: Uint8Array; riders: Rider[]; tick: number; rng: number };

const DX = [0, 1, 0, -1], DY = [-1, 0, 1, 0];
export const opposite = (d: Dir) => ((d + 2) % 4) as Dir;
const turn = (d: Dir, by: 1 | 3) => ((d + by) % 4) as Dir;

/** mulberry32: small, seedable, good enough for a game. */
function random(g: Game) {
  g.rng = (g.rng + 0x6D2B79F5) | 0;
  let t = g.rng;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}

const open = (g: Game, x: number, y: number) => x >= 0 && y >= 0 && x < g.w && y < g.h && g.cells[y * g.w + x] === 0;

export function newGame(seed: number, w = 60, h = 36): Game {
  const g: Game = { w, h, cells: new Uint8Array(w * h), tick: 0, rng: seed | 0, riders: [
    { x: w >> 1, y: h - 4, dir: 0, alive: true, diedAt: null },
    { x: 4, y: h >> 1, dir: 1, alive: true, diedAt: null },
    { x: w - 5, y: h >> 1, dir: 3, alive: true, diedAt: null },
  ] };
  // Patches of the Static, then a clear run-up ahead of every courier.
  for (let i = 0; i < 9; i++) {
    const px = 3 + Math.floor(random(g) * (w - 6)), py = 3 + Math.floor(random(g) * (h - 6)), size = 2 + Math.floor(random(g) * 2);
    for (let y = py; y < py + size; y++) for (let x = px; x < px + size; x++) if (x < w && y < h) g.cells[y * w + x] = 1;
  }
  g.riders.forEach((r, i) => {
    for (let s = 0; s < 10; s++) for (let side = -1; side <= 1; side++) {
      const x = r.x + DX[r.dir] * s + DY[r.dir] * side, y = r.y + DY[r.dir] * s + DX[r.dir] * side;
      if (x >= 0 && y >= 0 && x < w && y < h) g.cells[y * w + x] = 0;
    }
    g.cells[r.y * w + r.x] = 2 + i;
  });
  return g;
}

/** Open cells reachable from (x, y), counted up to `limit`. */
function room(g: Game, x: number, y: number, limit: number) {
  if (!open(g, x, y)) return 0;
  const seen = new Set([y * g.w + x]), queue = [y * g.w + x];
  while (queue.length && seen.size < limit) {
    const at = queue.pop()!;
    for (let d = 0; d < 4; d++) {
      const nx = (at % g.w) + DX[d], ny = Math.floor(at / g.w) + DY[d], key = ny * g.w + nx;
      if (open(g, nx, ny) && !seen.has(key)) { seen.add(key); queue.push(key); }
    }
  }
  return seen.size;
}

/** A computer courier keeps going while there's room, turns toward the most open space, and now and then turns on a whim. */
export function botDir(g: Game, r: Rider): Dir {
  const whim = random(g) < 0.04 ? (random(g) < 0.5 ? 1 : 3) : 0;
  let best = r.dir, bestScore = -1;
  for (const d of [r.dir, turn(r.dir, 1), turn(r.dir, 3)]) {
    const score = room(g, r.x + DX[d], r.y + DY[d], 200) + (d === r.dir ? 3 : 0) + (whim && d === turn(r.dir, whim) ? 6 : 0);
    if (score > bestScore) { best = d; bestScore = score; }
  }
  return best;
}

/** One tick: the player turns (a reverse is ignored), the bots choose, then everyone moves at once.
 *  A courier that hits a wall, a trail or the Static is lost; two heads meeting in one cell are both lost. */
export function step(g: Game, playerTurn?: Dir) {
  const [player] = g.riders;
  if (playerTurn !== undefined && playerTurn !== opposite(player.dir)) player.dir = playerTurn;
  g.riders.forEach((r, i) => { if (i > 0 && r.alive) r.dir = botDir(g, r); });
  const next = g.riders.map(r => ({ x: r.x + DX[r.dir], y: r.y + DY[r.dir] }));
  const lost = g.riders.map((r, i) => r.alive && (!open(g, next[i].x, next[i].y)
    || g.riders.some((o, j) => j !== i && o.alive && next[j].x === next[i].x && next[j].y === next[i].y)));
  g.tick++;
  g.riders.forEach((r, i) => {
    if (!r.alive) return;
    if (lost[i]) { r.alive = false; r.diedAt = g.tick; return; }
    r.x = next[i].x; r.y = next[i].y;
    g.cells[r.y * g.w + r.x] = 2 + i;
  });
}

/** Milliseconds per tick: it starts calm and speeds up to a cap. */
export const tickMs = (tick: number) => Math.max(45, 110 - tick * 0.12);
