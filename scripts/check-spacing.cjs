// Every gap, padding and margin in the web stylesheets is on the docs/DESIGN.md spacing scale.
// node scripts/check-spacing.cjs
const fs = require("node:fs");
const path = require("node:path");
const ok = px => px <= 3 || [4, 8, 12, 16, 24, 32, 48].includes(px) || (px > 48 && px % 8 === 0);
const files = ["apps/web/app/globals.css", ...fs.readdirSync("apps/web/styles").map(f => path.join("apps/web/styles", f))];
const bad = [];
for (const file of files.filter(f => f.endsWith(".css"))) {
  for (const [, prop, value] of fs.readFileSync(file, "utf8").matchAll(/((?:row-|column-)?gap|(?:padding|margin)[a-z-]*)\s*:\s*([^;}]+)/g)) {
    if (value.includes("calc(")) continue;
    for (const [, n] of value.matchAll(/(?<![-\w.(])(\d+)px/g)) if (!ok(+n)) bad.push(`${file}: ${prop}: ${value.trim()}`);
  }
}
if (bad.length) { console.error(`Off the spacing scale (4 8 12 16 24 32 48):\n${bad.join("\n")}`); process.exit(1); }
console.log("spacing ok");
