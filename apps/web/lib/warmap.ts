import type { Faction } from "./factions";
import type { Genre } from "./war";

/**
 * War map geometry. Each genre owns one cell of a coarse hex grid (`map` from the API, axial q/r).
 * The land is drawn as small hex tiles: every tile goes to the nearest territory or sea cell,
 * with smooth noise so borders and coasts look hand-drawn. Pure integer hashing keeps the
 * result identical on the server and in the browser.
 */
export const TILE = 8;
const TW = TILE * Math.sqrt(3), TROW = TILE * 1.5;
const CELL = TW * 7, CROW = CELL * Math.sqrt(3) / 2;
const MARGIN = 1.6, WOBBLE = CELL * .3, NOISE = 30;
const DIRS = [[1, 0], [1, -1], [0, -1], [-1, 0], [-1, 1], [0, 1]] as const;

export type Territory = {
  genre: Genre; x: number; y: number; tones: [string, string, string]; outline: string;
  box: { x: number; y: number; w: number; h: number };
};
export type WarGeometry = {
  width: number; height: number; territories: Territory[]; sea: string; shallows: string;
  coast: string; borders: string; frontiers: string; regions: { home: Faction; x: number; y: number }[];
  /** Open sky beside Aetheron's region name, where the Pale Moon hangs. */
  moon: { x: number; y: number } | null;
};

function hash(x: number, y: number, seed: number) {
  let h = Math.imul(x, 374761393) ^ Math.imul(y, 668265263) ^ Math.imul(seed, 1274126177);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
function noise(px: number, py: number, seed: number) {
  const fx = px / NOISE, fy = py / NOISE, ix = Math.floor(fx), iy = Math.floor(fy);
  const sx = (fx - ix) * (fx - ix) * (3 - 2 * (fx - ix)), sy = (fy - iy) * (fy - iy) * (3 - 2 * (fy - iy));
  const top = hash(ix, iy, seed) * (1 - sx) + hash(ix + 1, iy, seed) * sx;
  const bottom = hash(ix, iy + 1, seed) * (1 - sx) + hash(ix + 1, iy + 1, seed) * sx;
  return (top * (1 - sy) + bottom * sy) * 2 - 1;
}
const cellX = (q: number, r: number) => CELL * (q + r / 2), cellY = (r: number) => CROW * r;
const corner = (x: number, y: number, k: number) => { const a = Math.PI / 180 * (60 * k - 30); return [x + TILE * Math.cos(a), y + TILE * Math.sin(a)] as const; };
const n1 = (v: number) => Math.round(v * 10) / 10;
const hexPath = (x: number, y: number) => `M${[0, 1, 2, 3, 4, 5].map(k => corner(x, y, k).map(n1).join(" ")).join("L")}Z`;
/** The two corners of a pointy-top hex that face direction d. */
const edgeCorners = DIRS.map(([a, b]) => { const angle = Math.atan2(TROW * b, TW * (a + b / 2)) * 180 / Math.PI; return [0, 1, 2, 3, 4, 5].filter(k => Math.abs(((60 * k - 30 - angle + 540) % 360) - 180) < 31); });
const edge = (x: number, y: number, d: number) => { const [a, b] = edgeCorners[d].map(k => corner(x, y, k).map(n1)); return `M${a[0]} ${a[1]}L${b[0]} ${b[1]}`; };

/** Genres without a map spot (added by staff later) take the free cells nearest the coast. */
function place(genres: Genre[]) {
  const taken = new Map<string, Genre>(), spots = new Map<string, { q: number; r: number }>();
  for (const g of genres) if (g.map) { taken.set(`${g.map.q},${g.map.r}`, g); spots.set(g.id, g.map); }
  for (const g of genres.filter(x => !x.map)) {
    // Fill the free cell closest to the middle of the map, so new land grows the continent compactly.
    const placed = [...spots.values()], mx = placed.reduce((n, p) => n + cellX(p.q, p.r), 0) / (placed.length || 1), my = placed.reduce((n, p) => n + cellY(p.r), 0) / (placed.length || 1);
    const coast: { q: number; r: number; score: number }[] = [];
    for (const { q, r } of placed) for (const [a, b] of DIRS) {
      const key = `${q + a},${r + b}`;
      if (!taken.has(key)) coast.push({ q: q + a, r: r + b, score: -Math.hypot(cellX(q + a, r + b) - mx, cellY(r + b) - my) * 1000 - (r + b) * 10 - (q + a) });
    }
    const spot = spots.size ? coast.sort((a, b) => b.score - a.score)[0] : { q: 0, r: 0 };
    taken.set(`${spot.q},${spot.r}`, g); spots.set(g.id, spot);
  }
  return spots;
}

export function warGeometry(genres: Genre[]): WarGeometry {
  if (!genres.length) return { width: 0, height: 0, territories: [], sea: "", shallows: "", coast: "", borders: "", frontiers: "", regions: [], moon: null };
  const spots = place(genres);
  const qs = [...spots.values()];
  const minQ = Math.min(...qs.map(s => s.q)) - 3, maxQ = Math.max(...qs.map(s => s.q)) + 3, minR = Math.min(...qs.map(s => s.r)) - 2, maxR = Math.max(...qs.map(s => s.r)) + 2;
  const land = genres.map((g, i) => { const s = spots.get(g.id)!; return { genre: g, x: cellX(s.q, s.r), y: cellY(s.r), seed: i + 1 }; });
  const occupied = new Set(qs.map(s => `${s.q},${s.r}`));
  const sea: { x: number; y: number; seed: number }[] = [];
  for (let r = minR; r <= maxR; r++) for (let q = minQ; q <= maxQ; q++) if (!occupied.has(`${q},${r}`)) sea.push({ x: cellX(q, r), y: cellY(r), seed: 1000 + sea.length });
  const xs = land.map(l => l.x), ys = land.map(l => l.y);
  const left = Math.min(...xs) - CELL * MARGIN, right = Math.max(...xs) + CELL * MARGIN, top = Math.min(...ys) - CROW * MARGIN, bottom = Math.max(...ys) + CROW * MARGIN;
  // Tile lattice (axial a/b) covering the frame; owner -1 is sea.
  const b0 = Math.floor(top / TROW), b1 = Math.ceil(bottom / TROW);
  const owner = new Map<string, number>(), tiles: { a: number; b: number; x: number; y: number }[] = [];
  for (let b = b0; b <= b1; b++) {
    const y = b * TROW, a0 = Math.floor(left / TW - b / 2), a1 = Math.ceil(right / TW - b / 2);
    for (let a = a0; a <= a1; a++) {
      const x = TW * (a + b / 2);
      let best = Infinity, who = -1;
      for (let i = 0; i < land.length; i++) { const c = land[i]; const d = Math.hypot(x - c.x, y - c.y) + WOBBLE * noise(x, y, c.seed); if (d < best) { best = d; who = i; } }
      for (const c of sea) { const d = Math.hypot(x - c.x, y - c.y) + WOBBLE * noise(x, y, c.seed); if (d < best) { best = d; who = -1; } }
      owner.set(`${a},${b}`, who); tiles.push({ a, b, x: x - left, y: y - top });
    }
  }
  // Noise can strand a few tiles away from their territory; fold those into whatever surrounds them.
  const seen = new Set<string>();
  for (const t of tiles) {
    const start = `${t.a},${t.b}`, who = owner.get(start)!;
    if (seen.has(start)) continue;
    const group = [start], around = new Map<number, number>();
    seen.add(start);
    for (let i = 0; i < group.length; i++) {
      const [a, b] = group[i].split(",").map(Number);
      for (const [da, db] of DIRS) {
        const key = `${a + da},${b + db}`, o = owner.get(key);
        if (o === undefined) continue;
        if (o !== who) around.set(o, (around.get(o) ?? 0) + 1);
        else if (!seen.has(key)) { seen.add(key); group.push(key); }
      }
    }
    if (group.length < 12 && around.size) { const into = [...around].sort((x, y) => y[1] - x[1])[0][0]; for (const key of group) owner.set(key, into); }
  }
  const tones = land.map(() => ["", "", ""] as [string, string, string]), outlines = land.map(() => "");
  const sums = land.map(() => ({ x: 0, y: 0, n: 0, minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity }));
  let seaPath = "", shallows = "", coast = "", borders = "", frontiers = "";
  for (const t of tiles) {
    const who = owner.get(`${t.a},${t.b}`)!, hex = hexPath(t.x, t.y);
    const around = DIRS.map(([da, db]) => owner.get(`${t.a + da},${t.b + db}`));
    if (who < 0) { if (around.some(o => o !== undefined && o >= 0)) shallows += hex; else seaPath += hex; continue; }
    tones[who][Math.floor(hash(t.a, t.b, 7) * 3)] += hex;
    const s = sums[who]; s.x += t.x; s.y += t.y; s.n++; s.minX = Math.min(s.minX, t.x); s.maxX = Math.max(s.maxX, t.x); s.minY = Math.min(s.minY, t.y); s.maxY = Math.max(s.maxY, t.y);
    around.forEach((o, d) => {
      if (o === who || o === undefined) return;
      outlines[who] += edge(t.x, t.y, d);
      if (o < 0) coast += edge(t.x, t.y, d);
      else if (o > who) { if (land[o].genre.home !== land[who].genre.home) frontiers += edge(t.x, t.y, d); else borders += edge(t.x, t.y, d); }
    });
  }
  const territories: Territory[] = land.map((l, i) => {
    const s = sums[i];
    // Anchor labels on the tile nearest the centre so they stay inside odd shapes.
    const cx = s.n ? s.x / s.n : l.x - left, cy = s.n ? s.y / s.n : l.y - top;
    const anchor = tiles.filter(t => owner.get(`${t.a},${t.b}`) === i).sort((p, q) => Math.hypot(p.x - cx, p.y - cy) - Math.hypot(q.x - cx, q.y - cy))[0] ?? { x: cx, y: cy };
    return { genre: l.genre, x: anchor.x, y: anchor.y, tones: tones[i], outline: outlines[i], box: { x: s.minX - TILE, y: s.minY - TILE, w: s.maxX - s.minX + TILE * 2, h: s.maxY - s.minY + TILE * 2 } };
  });
  const isSea = (x: number, y: number) => { const b = Math.round((y + top) / TROW), a = Math.round((x + left) / TW - b / 2); return (owner.get(`${a},${b}`) ?? -1) < 0; };
  const regions = (["aetheron", "glint", "myria"] as Faction[]).flatMap(home => {
    const own = territories.filter(t => t.genre.home === home);
    if (!own.length) return [];
    const x = own.reduce((n, t) => n + t.x, 0) / own.length;
    // Region names sit in open water next to their region: above, below, then beside it.
    const minX = Math.min(...own.map(t => t.box.x)), maxX = Math.max(...own.map(t => t.box.x + t.box.w)), minY = Math.min(...own.map(t => t.box.y)), maxY = Math.max(...own.map(t => t.box.y + t.box.h));
    const spots = [[x, minY - 12], [x, maxY + 20], [minX - 70, (minY + maxY) / 2], [maxX + 70, (minY + maxY) / 2], [x, minY - 30], [x, maxY + 38]];
    const open = ([sx, sy]: number[]) => sx > 60 && sy > 16 && sx < right - left - 60 && sy < bottom - top - 8 && [-60, -30, 0, 30, 60].every(dx => [-12, 0, 4].every(dy => isSea(sx + dx, sy + dy)));
    const spot = spots.find(open);
    return spot ? [{ home, x: spot[0], y: spot[1] }] : [];
  });
  // The page overlays controls (top left), the title (top right), the key (bottom left) and the compass (bottom right).
  const W = right - left, H = bottom - top, overlaid = (mx: number, my: number) => (my < 70 && (mx < 170 || mx > W - 250)) || (my > H - 70 && (mx < 400 || mx > W - 90));
  const clear = ([mx, my]: number[]) => mx > 30 && my > 30 && mx < W - 30 && my < H - 30 && !overlaid(mx, my) && [0, 1, 2, 3, 4, 5, 6, 7].every(k => isSea(mx + 30 * Math.cos(k * Math.PI / 4), my + 30 * Math.sin(k * Math.PI / 4))) && [-45, -22, 0, 22, 45].every(dx => isSea(mx + dx, my + 30)) && regions.every(r => Math.abs(r.x - mx) > 110 || Math.abs(r.y - my) > 36);
  const home = territories.filter(t => t.genre.home === "aetheron");
  const hx = home.reduce((n, t) => n + t.x, 0) / (home.length || 1), hy = home.reduce((n, t) => n + t.y, 0) / (home.length || 1);
  const rings = [140, 180, 220, 260].flatMap(rad => [200, 230, 250, 270, 290, 320, 170, 140].map(deg => [hx + rad * Math.cos(deg * Math.PI / 180), hy + rad * Math.sin(deg * Math.PI / 180)]));
  const moon = home.length ? rings.find(clear) : undefined;
  return { width: n1(right - left), height: n1(bottom - top), territories, sea: seaPath, shallows, coast, borders, frontiers, regions, moon: moon ? { x: moon[0], y: moon[1] } : null };
}
