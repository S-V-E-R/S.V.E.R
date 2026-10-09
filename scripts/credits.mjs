// Builds apps/web/app/credits/credits.json: every shipped Rust crate (normal dependencies of the
// API on Linux) and npm package (web production dependencies), with its license and license text,
// plus SRS, PostgreSQL and the fonts (docs/CHANNEL_ADDITIONS.md "Credits").
//   node scripts/credits.mjs          regenerate
//   node scripts/credits.mjs --check  fail if a dependency changed without regenerating (CI)
// For a choice that includes MIT ("MIT OR Apache-2.0") S.V.E.R uses MIT, so only that notice is
// kept. Identical texts are stored once.
import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "apps/web/app/credits/credits.json");
const run = (command, cwd) => execSync(command, { cwd, maxBuffer: 1 << 28, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });

function rust() {
  const meta = JSON.parse(run("cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu", join(root, "apps/api")));
  const packages = new Map(meta.packages.map(p => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map(n => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const seen = new Set();
  const stack = [...members];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id).deps) if (dep.dep_kinds.some(k => k.kind === null)) stack.push(dep.pkg);
  }
  return [...seen].filter(id => !members.has(id)).map(id => packages.get(id)).map(p => ({
    kind: "rust", name: p.name, version: p.version, license: p.license ?? "See license text",
    url: p.repository ?? p.homepage ?? `https://crates.io/crates/${p.name}`, dir: dirname(p.manifest_path),
  }));
}

function npm() {
  const listed = JSON.parse(run(`${process.env.PNPM ?? "corepack pnpm"} licenses list --json --prod`, join(root, "apps/web")));
  // Platform builds of native tools (`@next/swc-linux-x64-gnu`, `@img/sharp-win32-x64`) differ per
  // machine and only run on the server or at build time, so the list stays the same everywhere.
  const native = /(^|[-/])(win32|linux|darwin|freebsd|android|libvips)(-|$)/;
  return Object.values(listed).flat().filter(p => !native.test(p.name)).flatMap(p => p.versions.map((version, i) => ({
    kind: "npm", name: p.name, version, license: p.license, url: p.homepage ?? `https://www.npmjs.com/package/${p.name}`, dir: p.paths[i],
  })));
}

const OTHER = [
  { kind: "other", name: "SRS (Simple Realtime Server)", version: "7", license: "MIT", url: "https://github.com/ossrs/srs", file: "LICENSE-SRS.txt" },
  { kind: "other", name: "PostgreSQL", version: "17", license: "PostgreSQL", url: "https://www.postgresql.org/about/licence/", file: "LICENSE-PostgreSQL.txt" },
  { kind: "other", name: "Cinzel (font)", version: "", license: "OFL-1.1", url: "https://github.com/NDISCOVER/Cinzel", file: "OFL-cinzel.txt" },
  { kind: "other", name: "Barlow and Barlow Condensed (fonts)", version: "", license: "OFL-1.1", url: "https://github.com/jpt/barlow", file: "OFL-barlow.txt" },
];

/** The license text a package ships: its MIT notice when MIT is offered, otherwise every license file. */
function licenseText(dir, license) {
  let files;
  try { files = readdirSync(dir, { withFileTypes: true }).filter(f => f.isFile() && /^(licen[cs]e|copying|unlicense)/i.test(f.name)).map(f => f.name); } catch { return null; }
  if (files.length === 0) return null;
  const mit = /\bMIT\b/.test(license) ? files.filter(f => /mit/i.test(f)) : [];
  const chosen = mit.length ? mit : files;
  return chosen.sort().map(f => readFileSync(join(dir, f), "utf8").replace(/\r\n/g, "\n").trim()).join("\n\n");
}

const key = p => `${p.kind}:${p.name}@${p.version}`;
const found = [...rust(), ...npm()].sort((a, b) => key(a).localeCompare(key(b)));

if (process.argv.includes("--check")) {
  const saved = JSON.parse(readFileSync(out, "utf8")).packages.filter(p => p.kind !== "other").map(key);
  const now = found.map(key);
  const added = now.filter(k => !saved.includes(k));
  const removed = saved.filter(k => !now.includes(k));
  if (added.length || removed.length) {
    console.error(`Credits are out of date. Run: node scripts/credits.mjs\n  added: ${added.join(", ") || "none"}\n  removed: ${removed.join(", ") || "none"}`);
    process.exit(1);
  }
  console.log(`Credits match ${now.length} shipped packages.`);
  process.exit(0);
}

const texts = {};
const store = text => {
  if (!text) return null;
  const hash = createHash("sha256").update(text).digest("hex").slice(0, 16);
  texts[hash] = text;
  return hash;
};
const packages = [
  ...OTHER.map(({ file, ...p }) => ({ ...p, text: store(readFileSync(join(root, "scripts/credits", file), "utf8").replace(/\r\n/g, "\n").trim()) })),
  ...found.map(({ dir, ...p }) => ({ ...p, text: store(licenseText(dir, p.license)) })),
];
writeFileSync(out, JSON.stringify({ packages, texts }, null, 1) + "\n");
console.log(`Wrote ${packages.length} packages and ${Object.keys(texts).length} license texts.`);
